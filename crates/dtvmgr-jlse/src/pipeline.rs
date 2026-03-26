//! Pipeline orchestration for the CM detection workflow.
//!
//! Executes all pipeline steps in order: input validation, channel
//! detection, parameter detection, external command execution, AVS
//! concatenation, chapter generation, and optional EIT-based MKV encoding.

use std::cell::Cell;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tracing::{debug, info, instrument, warn};

use crate::avs;
use crate::channel;
use crate::command;
use crate::command::ffmpeg::MkvMetadata;
use crate::output;
use crate::param;
use crate::progress::{self, ProgressEvent, ProgressMode};
use crate::settings::{BinaryPaths, DataPaths, OutputPaths, init_output_paths};
use crate::storage;
use crate::types::{AvsTarget, Channel, DetectionParam, JlseConfig, JlseDirs, JlseEncode};
use crate::validate;

// ── Types ────────────────────────────────────────────────────

/// Pipeline execution context.
#[allow(clippy::module_name_repetitions, clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct PipelineContext {
    /// Input TS file path.
    pub input: PathBuf,
    /// Channel name override.
    pub channel_name: Option<String>,
    /// JLSE configuration.
    pub config: JlseConfig,
    /// Run ffmpeg encoding.
    pub encode: bool,
    /// Encode target AVS selection.
    pub target: AvsTarget,
    /// Add chapter metadata to encoded file.
    pub add_chapter: bool,
    /// Output directory override.
    pub out_dir: Option<PathBuf>,
    /// Output filename override (without extension).
    pub out_name: Option<String>,
    /// Output extension override (e.g. "mp4").
    /// When set, takes precedence over `config.encode.format`.
    pub out_extension: Option<String>,
    /// Remove intermediate files after processing.
    pub remove: bool,
    /// Progress output mode (e.g. `EPGStation`).
    pub progress_mode: Option<ProgressMode>,
    /// Skip pre-encode duration validation.
    pub skip_duration_check: bool,
    /// Force re-run of all pipeline steps, ignoring cached results.
    pub force: bool,
}

/// Accumulated pipeline metrics emitted as a structured log on completion.
#[derive(Debug, Default)]
struct PipelineSummary {
    /// Output file path.
    output: Option<PathBuf>,
    /// Video codec used.
    codec: Option<String>,
    /// Resolved quality value (CRF / ICQ).
    quality_value: Option<f32>,
    /// Quality parameter flag.
    quality_param: Option<String>,
    /// VMAF score from quality search.
    vmaf: Option<f32>,
    /// VMAF `n_subsample` setting.
    vmaf_subsample: Option<u32>,
    /// Output file size in bytes.
    output_size: Option<u64>,
    /// Input TS duration in seconds.
    ts_duration_secs: Option<f64>,
    /// AVS (content) duration in seconds.
    avs_duration_secs: Option<f64>,
    /// Content ratio (AVS / TS) as percentage.
    ratio_percent: Option<f64>,
    /// Post-encode video stream duration.
    post_video_secs: Option<f64>,
    /// Post-encode audio stream duration.
    post_audio_secs: Option<f64>,
}

/// Configuration snapshot stored in the quality search cache.
///
/// Used to detect stale cache entries when settings change between runs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
struct QualitySearchCacheConfig {
    codec: String,
    quality_param: String,
    min_quality: f32,
    max_quality: f32,
    quality_increment: f32,
    quality_hint: f32,
    preset: Option<String>,
    pix_fmt: Option<String>,
    video_filter: String,
    reference_filter: Option<String>,
    target_vmaf: f32,
    max_encoded_percent: f32,
    min_vmaf_tolerance: f32,
    thorough: bool,
    sample_duration_secs: f64,
    skip_secs: f64,
    sample_every_secs: f64,
    min_samples: u32,
    max_samples: u32,
    vmaf_subsample: u32,
    extra_encode_args: Vec<String>,
    extra_input_args: Vec<String>,
    /// Raw content of `obs_cut.avs` — encodes both segment positions and
    /// filter state, so any CM detection change invalidates the cache.
    avs_content: String,
}

/// On-disk format for `obs_quality_search.json`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct QualitySearchCache {
    config: QualitySearchCacheConfig,
    result: dtvmgr_vmaf::SearchResult,
}

// ── Pipeline ─────────────────────────────────────────────────

/// Run the full CM detection pipeline.
///
/// # Errors
///
/// Returns an error if any pipeline step fails (validation, file I/O,
/// external command execution, etc.).
#[allow(clippy::module_name_repetitions)]
#[instrument(skip_all, err(level = "error"))]
pub fn run_pipeline(
    ctx: &PipelineContext,
    on_progress: Option<&dyn Fn(ProgressEvent)>,
) -> Result<()> {
    let mut summary = PipelineSummary::default();
    let result = run_pipeline_inner(ctx, on_progress, &mut summary);

    let input = &ctx.input;
    let encode_args = JlseEncode::build_encode_args(ctx.config.encode.as_ref()).1;

    let status = if result.is_ok() {
        "completed"
    } else {
        "failed"
    };
    emit_pipeline_summary(input, &encode_args, &summary, status);

    if let Err(ref e) = result {
        warn!(error = format!("{e:#}"), "pipeline failed");
    }
    result
}

/// Inner pipeline implementation.
#[allow(
    clippy::module_name_repetitions,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
fn run_pipeline_inner(
    ctx: &PipelineContext,
    on_progress: Option<&dyn Fn(ProgressEvent)>,
    summary: &mut PipelineSummary,
) -> Result<()> {
    // Step 1: Validate input extension and encode config
    validate_input(&ctx.input)?;
    validate_encode_config(ctx.config.encode.as_ref())?;

    // Canonicalize all directory paths up-front so every downstream
    // step (AVS generation, external commands, ffmpeg) receives
    // absolute paths — matching the Node.js `path.resolve` / `__dirname`
    // behaviour.
    let input = ctx
        .input
        .canonicalize()
        .with_context(|| format!("failed to canonicalize input path: {}", ctx.input.display()))?;
    let dirs = canonicalize_dirs(&ctx.config.dirs)?;

    let filename = input
        .file_stem()
        .context("input file has no stem")?
        .to_string_lossy()
        .into_owned();
    info!(input = %input.display(), "starting pipeline");

    storage::log_storage_stats(
        input.parent().unwrap_or_else(|| Path::new("/")),
        ctx.out_dir.as_deref(),
    );

    let config_abs = JlseConfig {
        dirs,
        bins: ctx.config.bins.clone(),
        encode: ctx.config.encode.clone(),
    };
    let bins = BinaryPaths::from_config(&config_abs);
    let data = DataPaths::from_config(&config_abs);
    let paths = init_output_paths(&config_abs.dirs.result, &filename)?;

    // Step 2: Channel detection (with PAT SID reverse lookup)
    let channels = channel::load_channels(&data.channel_list)?;
    let filepath = input.to_string_lossy();
    let pat_sids = extract_pat_sids(&bins.tstables, &input);
    let detected_channel = channel::detect_channel_with_sid(
        &channels,
        &filepath,
        ctx.channel_name.as_deref(),
        pat_sids.as_deref(),
    );
    if let Some(ref ch) = detected_channel {
        info!(short = %ch.short, "detected channel");
    } else {
        info!("no channel detected");
    }

    // Step 3: Parameter detection
    let params_jl1 = param::load_params(&data.param_jl1)?;
    let params_jl2 = param::load_params(&data.param_jl2)?;
    let det_param = param::detect_param(
        &params_jl1,
        &params_jl2,
        detected_channel.as_ref(),
        &filepath,
    );
    info!(jl_run = %det_param.jl_run, "detected parameters");

    // Step 4: Write obs_param.txt
    write_obs_param(&paths.obs_param_path, detected_channel.as_ref(), &det_param)?;

    // Step 5: AVS generation
    avs::create(&paths.input_avs, &input, avs::STREAM_INDEX_NORMAL)?;
    debug!(path = %paths.input_avs.display(), "created input AVS");

    // Determine total stage count (quality search adds one stage)
    let quality_search_enabled = ctx
        .config
        .encode
        .as_ref()
        .and_then(|e| e.quality_search.as_ref())
        .is_some_and(|qs| qs.enabled);
    let total_stages: u8 = if quality_search_enabled { 5 } else { 4 };

    // Step 6: chapter_exe (stages 1-2: lwi index + mute detection)
    if !ctx.force && paths.chapterexe_output.exists() {
        info!(
            path = %paths.chapterexe_output.display(),
            "chapter_exe output exists, skipping (use --force to re-run)"
        );
    } else if let Some(cb) = on_progress {
        cb(ProgressEvent::StageStart {
            stage: 1,
            total: total_stages,
            name: "lwi index".to_owned(),
        });
        let in_mute_phase = Cell::new(false);
        let total_frames = Cell::new(0u32);
        let enter_mute_phase = || {
            if !in_mute_phase.get() {
                in_mute_phase.set(true);
                cb(ProgressEvent::StageStart {
                    stage: 2,
                    total: total_stages,
                    name: "chapter_exe".to_owned(),
                });
            }
        };
        let on_log = |line: &str| {
            cb(ProgressEvent::Log(line.to_owned()));
            if let Some(pct) = progress::parse_lwi_percent(line) {
                let stage_pct = f64::from(pct.min(100)) / 100.0;
                cb(ProgressEvent::StageProgress {
                    percent: stage_pct,
                    log: line.to_owned(),
                });
            } else if let Some(total) = progress::parse_video_frames_total(line) {
                total_frames.set(total);
                enter_mute_phase();
            } else if let Some(current) = progress::parse_mute_frame(line) {
                enter_mute_phase();
                let total = total_frames.get();
                if total > 0 {
                    let pct = f64::from(current.min(total)) / f64::from(total);
                    cb(ProgressEvent::StageProgress {
                        percent: pct,
                        log: line.to_owned(),
                    });
                }
            }
        };
        command::chapter_exe::run_logged(
            &bins.chapter_exe,
            &paths.input_avs,
            &paths.chapterexe_output,
            &on_log,
        )?;
    } else {
        command::chapter_exe::run(
            &bins.chapter_exe,
            &paths.input_avs,
            &paths.chapterexe_output,
        )?;
    }
    debug!("chapter_exe completed");

    // Step 7: logoframe (stage 3)
    if !ctx.force && paths.logoframe_txt_output.exists() {
        info!(
            path = %paths.logoframe_txt_output.display(),
            "logoframe output exists, skipping (use --force to re-run)"
        );
    } else if let Some(cb) = on_progress {
        cb(ProgressEvent::StageStart {
            stage: 3,
            total: total_stages,
            name: "logoframe".to_owned(),
        });
        let on_log = |line: &str| {
            cb(ProgressEvent::Log(line.to_owned()));
            if let Some(pct) = progress::parse_lwi_percent(line) {
                let stage_pct = f64::from(pct.min(100)) / 100.0 * 0.2;
                cb(ProgressEvent::StageProgress {
                    percent: stage_pct,
                    log: line.to_owned(),
                });
            } else if let Some((current, total)) = progress::parse_logoframe_checking(line)
                && total > 0
            {
                let pct = f64::from(current.min(total)) / f64::from(total);
                cb(ProgressEvent::StageProgress {
                    percent: pct.mul_add(0.8, 0.2),
                    log: format!("checking {current}/{total}"),
                });
            }
        };
        command::logoframe::run_logged(
            &bins.logoframe,
            &paths.input_avs,
            &paths.logoframe_txt_output,
            &paths.logoframe_avs_output,
            &config_abs.dirs.logo,
            detected_channel.as_ref(),
            &on_log,
        )?;
    } else {
        command::logoframe::run(
            &bins.logoframe,
            &paths.input_avs,
            &paths.logoframe_txt_output,
            &paths.logoframe_avs_output,
            &config_abs.dirs.logo,
            detected_channel.as_ref(),
        )?;
    }
    debug!("logoframe completed");

    // Step 8: join_logo_scp
    let jl_command_file = config_abs.dirs.jl.join(&det_param.jl_run);
    if !ctx.force && paths.output_avs_cut.exists() {
        info!(
            path = %paths.output_avs_cut.display(),
            "join_logo_scp output exists, skipping (use --force to re-run)"
        );
    } else if let Some(cb) = on_progress {
        let on_log = |line: &str| cb(ProgressEvent::Log(line.to_owned()));
        command::join_logo_scp::run_logged(
            &bins.join_logo_scp,
            &paths.logoframe_txt_output,
            &paths.chapterexe_output,
            &jl_command_file,
            &paths.output_avs_cut,
            &paths.jlscp_output,
            &det_param,
            &on_log,
        )?;
    } else {
        command::join_logo_scp::run(
            &bins.join_logo_scp,
            &paths.logoframe_txt_output,
            &paths.chapterexe_output,
            &jl_command_file,
            &paths.output_avs_cut,
            &paths.jlscp_output,
            &det_param,
        )?;
    }
    debug!("join_logo_scp completed");

    // Step 9: AVS concatenation
    output::avs::create_cutcm(
        &paths.output_avs_in_cut,
        &paths.input_avs,
        &paths.output_avs_cut,
    )?;
    output::avs::create_cutcm_logo(
        &paths.output_avs_in_cut_logo,
        &paths.input_avs,
        &paths.logoframe_avs_output,
        &paths.output_avs_cut,
    )?;
    debug!("AVS concatenation completed");

    // Step 10: Chapter generation
    generate_chapters(&paths)?;

    // Resolve FFmpeg encode args (always logged for visibility)
    let (input_encode_args, mut output_encode_args) =
        JlseEncode::build_encode_args(ctx.config.encode.as_ref());

    // Step 11.5: (optional) VMAF-based quality parameter search
    if let Some(quality_result) = run_quality_search(
        ctx.config.encode.as_ref(),
        &input,
        &paths.output_avs_cut,
        &bins.ffmpeg,
        &paths.quality_search_cache,
        ctx.force,
        total_stages,
        on_progress,
    )? {
        summary.vmaf = Some(quality_result.mean_vmaf);
        summary.quality_value = Some(quality_result.quality_value);
        summary.quality_param = Some(quality_result.quality_param.clone());
        inject_quality_override(&mut output_encode_args, &quality_result);
    }

    // Populate codec from config
    summary.codec = ctx
        .config
        .encode
        .as_ref()
        .and_then(|e| e.video.as_ref())
        .and_then(|v| v.codec.clone());
    summary.vmaf_subsample = ctx
        .config
        .encode
        .as_ref()
        .and_then(|e| e.quality_search.as_ref())
        .and_then(|qs| qs.vmaf_subsample);

    info!(input_args = ?input_encode_args, output_args = ?output_encode_args, "ffmpeg encode args");

    // Step 12: (optional) encoding
    if ctx.encode {
        // Pre-encode free space check on the output directory.
        if let Some(ref out_dir) = ctx.out_dir {
            let input_size = std::fs::metadata(&input).map(|m| m.len()).unwrap_or(0);
            if input_size > 0
                && let Some(free) = storage::free_bytes(out_dir)
            {
                if free < input_size {
                    bail!(
                        "insufficient disk space on {}: {} free < {} input size",
                        out_dir.display(),
                        free,
                        input_size,
                    );
                }
                debug!(
                    out_dir = %out_dir.display(),
                    free_bytes = free,
                    input_bytes = input_size,
                    "output directory has sufficient free space",
                );
            }
        }

        // Step 12a: EIT extraction for MKV metadata
        let mut mkv_metadata = extract_eit_for_mkv(&bins.tstables, &input, &paths.save_dir)
            .context("EIT extraction is required for MKV encoding")?;

        // Set ENCODER_SETTINGS from encode config, reflecting any quality search override.
        mkv_metadata.encoder_settings = ctx.config.encode.as_ref().map(|e| {
            let mut s = e.encoder_settings_summary();
            // Replace quality param with the actual value found by quality search.
            if let (Some(param), Some(val)) = (&summary.quality_param, summary.quality_value) {
                let param_name = param.trim_start_matches('-');
                s = replace_quality_token(&s, param_name, val);
            }
            // Insert VMAF score right after the quality token.
            if let Some(vmaf_score) = summary.vmaf {
                s = insert_token_after_quality(&s, &format!("vmaf {vmaf_score:.3}"));
            }
            s
        });

        let avs_file = select_avs_file(&paths, ctx.target);

        // Collect TS/AVS durations for summary (even before validation)
        summary.ts_duration_secs = command::ffprobe::duration(&bins.ffprobe, &input).ok();
        summary.avs_duration_secs = command::ffprobe::duration(&bins.ffprobe, avs_file).ok();
        if let (Some(ts), Some(avs)) = (summary.ts_duration_secs, summary.avs_duration_secs)
            && ts > 0.0
        {
            summary.ratio_percent = Some(avs / ts * 100.0);
        }

        // Pre-encode duration validation (also returns AVS duration for progress)
        let avs_duration = if ctx.skip_duration_check {
            info!("skipping pre-encode duration check");
            None
        } else {
            let rules = ctx
                .config
                .encode
                .as_ref()
                .and_then(|e| e.duration_check.as_deref());
            let dur = validate::check_pre_encode_duration(&bins.ffprobe, &input, avs_file, rules)
                .context("pre-encode duration check failed")?;
            Some(dur)
        };

        let extension = ctx
            .out_extension
            .as_deref()
            .or_else(|| ctx.config.encode.as_ref().and_then(|e| e.format.as_deref()))
            .unwrap_or("mkv");
        let output_file = resolve_output_path(
            &input,
            ctx.out_dir.as_deref(),
            ctx.out_name.as_deref(),
            extension,
        );

        let input_options = input_encode_args.join(" ");
        let extra_options = output_encode_args.join(" ");

        info!("running ffmpeg encoding");
        let chapter_file = if ctx.add_chapter {
            Some(paths.file_txt_cpt_cut.as_path())
        } else {
            None
        };

        if let Some(cb) = on_progress {
            cb(ProgressEvent::StageStart {
                stage: total_stages,
                total: total_stages,
                name: "FFmpeg".to_owned(),
            });
            let duration = avs_duration.unwrap_or_else(|| {
                command::ffprobe::duration(&bins.ffprobe, avs_file).unwrap_or(0.0)
            });
            command::ffmpeg::run_with_progress(
                &bins.ffmpeg,
                avs_file,
                &output_file,
                chapter_file,
                &mkv_metadata,
                &input_options,
                &extra_options,
                duration,
                cb,
            )?;
        } else {
            command::ffmpeg::run(
                &bins.ffmpeg,
                avs_file,
                &output_file,
                chapter_file,
                &mkv_metadata,
                &input_options,
                &extra_options,
            )?;
        }
        debug!("ffmpeg encoding completed");

        // Collect output file size
        summary.output = Some(output_file.clone());
        summary.output_size = std::fs::metadata(&output_file).ok().map(|m| m.len());

        // Collect post-encode stream durations
        summary.post_video_secs =
            command::ffprobe::stream_duration(&bins.ffprobe, &output_file, "v:0")
                .ok()
                .flatten();
        summary.post_audio_secs =
            command::ffprobe::stream_duration(&bins.ffprobe, &output_file, "a:0")
                .ok()
                .flatten();

        // Post-encode duration validation
        if !ctx.skip_duration_check {
            validate::check_post_encode_duration(&bins.ffprobe, &output_file)
                .context("post-encode duration check failed")?;
        }

        // Save EIT XML alongside encoded output
        if let Some(ref eit_src) = mkv_metadata.eit_xml_path {
            let eit_dest = output_file.with_extension("eit.xml");
            std::fs::copy(eit_src, &eit_dest)
                .with_context(|| format!("failed to copy EIT XML to {}", eit_dest.display()))?;
            debug!(path = %eit_dest.display(), "saved EIT XML alongside output");
        }
    }

    // Step 13: (optional) Remove intermediate files
    if ctx.remove {
        info!(dir = %paths.save_dir.display(), "removing intermediate files");
        std::fs::remove_dir_all(&paths.save_dir).with_context(|| {
            format!(
                "failed to remove intermediate directory: {}",
                paths.save_dir.display()
            )
        })?;
        debug!("intermediate files removed");
    }

    if let Some(cb) = on_progress {
        cb(ProgressEvent::Finished);
    }

    info!("pipeline completed successfully");

    Ok(())
}

/// Extract EIT from the middle of a TS file and build MKV metadata.
///
/// Steps:
/// 1. Extract a chunk from the file's midpoint.
/// 2. Run `tstables` to parse EIT p/f tables.
/// 3. Detect the recording target program.
/// 4. Save the raw EIT XML to `save_dir/eit.xml`.
/// 5. Build `MkvMetadata` from the detected program.
fn extract_eit_for_mkv(tstables_bin: &Path, input: &Path, save_dir: &Path) -> Result<MkvMetadata> {
    info!("extracting EIT metadata for MKV");

    let (target, xml) = dtvmgr_tsduck::detect_target_from_middle(tstables_bin, input)
        .context("failed to detect recording target from EIT")?;

    let target = target.context("no recording target found in EIT data")?;

    // Save EIT XML for attachment
    let eit_xml_path = save_dir.join("eit.xml");
    std::fs::write(&eit_xml_path, &xml)
        .with_context(|| format!("failed to write EIT XML: {}", eit_xml_path.display()))?;
    debug!(path = %eit_xml_path.display(), "saved EIT XML");

    let program = &target.program;
    info!(
        method = ?target.detection_method,
        program_name = ?program.program_name,
        "detected recording target"
    );

    Ok(MkvMetadata {
        title: program.program_name.clone(),
        subtitle: program.description.clone(),
        description: program.extended(),
        genre: program
            .genre1
            .and_then(dtvmgr_tsduck::eit::decode_genre)
            .map(ToOwned::to_owned),
        date_recorded: Some(program.start_time.clone()),
        encoder_settings: None,
        eit_xml_path: Some(eit_xml_path),
    })
}

/// Read `obs_cut.avs` and `obs_jlscp.txt`, generate chapter files.
fn generate_chapters(paths: &OutputPaths) -> Result<()> {
    let cut_content = std::fs::read_to_string(&paths.output_avs_cut)
        .with_context(|| format!("failed to read {}", paths.output_avs_cut.display()))?;
    let jlscp_content = std::fs::read_to_string(&paths.jlscp_output)
        .with_context(|| format!("failed to read {}", paths.jlscp_output.display()))?;

    let trims = output::chapter::parse_trims(&cut_content);
    let entries = output::chapter::parse_jlscp(&jlscp_content);
    let chapters = output::chapter::create_chapters(&trims, &entries);

    output::chapter::write_all(
        &chapters,
        &paths.file_txt_cpt_org,
        &paths.file_txt_cpt_cut,
        &paths.file_txt_cpt_tvt,
    )?;

    info!("chapter generation completed");
    Ok(())
}

/// Extract all service IDs from the PAT of a TS file.
///
/// Runs `tstables` to extract PAT XML, then parses all service IDs.
/// Returns `None` on any failure (non-fatal for the pipeline).
fn extract_pat_sids(tstables_bin: &Path, input: &Path) -> Option<Vec<u32>> {
    let xml = match dtvmgr_tsduck::command::extract_pat(tstables_bin, input) {
        Ok(xml) => xml,
        Err(e) => {
            debug!(error = %e, "PAT extraction failed, skipping SID lookup");
            return None;
        }
    };

    match dtvmgr_tsduck::pat::parse_pat_all_service_ids(&xml) {
        Ok(sids) if sids.is_empty() => {
            debug!("no service IDs found in PAT");
            None
        }
        Ok(sids) => {
            debug!(?sids, "extracted service IDs from PAT");
            Some(sids)
        }
        Err(e) => {
            debug!(error = %e, "PAT XML parsing failed, skipping SID lookup");
            None
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────

/// Canonicalize all directory paths in [`JlseDirs`] to absolute paths.
///
/// `AviSynth` and external commands require absolute paths for reliable
/// file resolution, especially with multibyte (Japanese) characters.
fn canonicalize_dirs(dirs: &JlseDirs) -> Result<JlseDirs> {
    let canon = |p: &Path, label: &str| -> Result<PathBuf> {
        p.canonicalize()
            .with_context(|| format!("failed to canonicalize {label} dir: {}", p.display()))
    };

    Ok(JlseDirs {
        jl: canon(&dirs.jl, "jl")?,
        logo: canon(&dirs.logo, "logo")?,
        result: canon(&dirs.result, "result")?,
    })
}

/// Validate that the input file has a `.ts` or `.m2ts` extension.
fn validate_input(input: &Path) -> Result<()> {
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ext {
        "ts" | "m2ts" => Ok(()),
        _ => bail!(
            "unsupported file extension: expected .ts or .m2ts, got {}",
            input.display()
        ),
    }
}

/// Write `obs_param` with channel and parameter information.
fn write_obs_param(path: &Path, channel: Option<&Channel>, param: &DetectionParam) -> Result<()> {
    let mut content = String::new();

    if let Some(ch) = channel {
        let _ = writeln!(content, "channel_recognize={}", ch.recognize);
        let _ = writeln!(content, "channel_short={}", ch.short);
        let _ = writeln!(content, "channel_service_id={}", ch.service_id);
    } else {
        let _ = writeln!(content, "channel_recognize=");
        let _ = writeln!(content, "channel_short=");
        let _ = writeln!(content, "channel_service_id=");
    }

    let _ = writeln!(content, "jl_run={}", param.jl_run);
    let _ = writeln!(content, "flags={}", param.flags);
    let _ = writeln!(content, "options={}", param.options);

    std::fs::write(path, &content)
        .with_context(|| format!("failed to write obs_param: {}", path.display()))?;

    debug!(path = %path.display(), "wrote obs_param.txt");
    Ok(())
}

/// Select the AVS file based on the encode target.
fn select_avs_file(paths: &OutputPaths, target: AvsTarget) -> &Path {
    match target {
        AvsTarget::CutCm => &paths.output_avs_in_cut,
        AvsTarget::CutCmLogo => &paths.output_avs_in_cut_logo,
    }
}

/// Resolve the output file path.
///
/// - `out_dir` overrides the parent directory of the input file.
/// - `out_name` overrides the file stem.
/// - `extension` sets the file extension (e.g. "mkv", "mp4").
pub fn resolve_output_path(
    input: &Path,
    out_dir: Option<&Path>,
    out_name: Option<&str>,
    extension: &str,
) -> PathBuf {
    let dir = out_dir.unwrap_or_else(|| input.parent().unwrap_or_else(|| Path::new(".")));
    let stem = out_name.map_or_else(
        || input.file_stem().unwrap_or_default().to_string_lossy(),
        std::borrow::Cow::Borrowed,
    );
    dir.join(format!("{stem}.{extension}"))
}

// ── Config validation ────────────────────────────────────────

/// Validate encode configuration for conflicting settings.
///
/// Checks that `quality_search.enabled = true` does not coexist
/// with quality parameters (`-crf`, `-global_quality`, etc.) in
/// `video.extra`.  Runs before any pipeline step so the user gets
/// immediate feedback.
fn validate_encode_config(encode: Option<&JlseEncode>) -> Result<()> {
    let Some(enc) = encode else {
        return Ok(());
    };

    // Reject VPP filter with redundant :format= (pix_fmt is the source of truth)
    if let Some(filter) = enc.video.as_ref().and_then(|v| v.filter.as_deref()) {
        super::command::ffmpeg::validate_vpp_no_format(filter)?;
    }

    let qs_enabled = enc.quality_search.as_ref().is_some_and(|qs| qs.enabled);
    if !qs_enabled {
        return Ok(());
    }

    if let Some(ref video) = enc.video {
        let conflicting: Vec<&str> = video
            .extra
            .iter()
            .filter(|arg| {
                dtvmgr_vmaf::QualityParam::ALL_FLAGS
                    .iter()
                    .any(|&flag| arg.as_str() == flag || arg.starts_with(&format!("{flag}:")))
            })
            .map(String::as_str)
            .collect();
        if !conflicting.is_empty() {
            bail!(
                "[jlse.encode.video] extra contains quality parameter(s) {conflicting:?} \
                 which conflict with [jlse.encode.quality_search]. \
                 Remove them from extra when quality_search.enabled = true"
            );
        }
    }

    Ok(())
}

// ── VMAF quality search cache helpers ────────────────────────

/// Build a [`QualitySearchCacheConfig`] from a resolved [`SearchConfig`] and
/// the raw `obs_cut.avs` content.
fn build_cache_config(
    config: &dtvmgr_vmaf::SearchConfig,
    avs_content: &str,
) -> QualitySearchCacheConfig {
    QualitySearchCacheConfig {
        codec: config.encoder.codec.clone(),
        quality_param: String::from(config.encoder.quality_param.flag()),
        min_quality: config.encoder.min_quality,
        max_quality: config.encoder.max_quality,
        quality_increment: config.encoder.quality_increment,
        quality_hint: config.encoder.quality_hint,
        preset: config.encoder.preset.clone(),
        pix_fmt: config.encoder.pix_fmt.clone(),
        video_filter: config.video_filter.clone(),
        reference_filter: config.reference_filter.clone(),
        target_vmaf: config.target_vmaf,
        max_encoded_percent: config.max_encoded_percent,
        min_vmaf_tolerance: config.min_vmaf_tolerance,
        thorough: config.thorough,
        sample_duration_secs: config.sample.duration_secs,
        skip_secs: config.sample.skip_secs,
        sample_every_secs: config.sample.sample_every_secs,
        min_samples: config.sample.min_samples,
        max_samples: config.sample.max_samples,
        vmaf_subsample: config.sample.vmaf_subsample,
        extra_encode_args: config.extra_encode_args.clone(),
        extra_input_args: config.extra_input_args.clone(),
        avs_content: avs_content.to_owned(),
    }
}

/// Try to load a cached quality search result from `path`.
///
/// Returns `None` if the file does not exist, is malformed, or the stored
/// config does not match `expected` (stale entry).
fn load_quality_cache(
    path: &Path,
    expected: &QualitySearchCacheConfig,
) -> Option<dtvmgr_vmaf::SearchResult> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return None;
    };
    let cache: QualitySearchCache = match serde_json::from_str(&content) {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "failed to parse quality cache");
            return None;
        }
    };
    if &cache.config != expected {
        info!(path = %path.display(), "quality search cache invalidated (config changed)");
        return None;
    }
    info!(
        quality_param = %cache.result.quality_param,
        quality_value = cache.result.quality_value,
        vmaf = cache.result.mean_vmaf,
        "loaded quality search result from cache"
    );
    Some(cache.result)
}

/// Write a quality search result to `path` as JSON.
///
/// Failures are logged as warnings and silently swallowed — a missing cache
/// file is not a fatal error.
fn save_quality_cache(
    path: &Path,
    config: &QualitySearchCacheConfig,
    result: &dtvmgr_vmaf::SearchResult,
) {
    let cache = QualitySearchCache {
        config: config.clone(),
        result: result.clone(),
    };
    match serde_json::to_string_pretty(&cache) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                warn!(path = %path.display(), error = %e, "failed to write quality cache");
            } else {
                debug!(path = %path.display(), "quality search result cached");
            }
        }
        Err(e) => warn!(error = %e, "failed to serialize quality cache"),
    }
}

// ── VMAF quality search integration ──────────────────────────

/// Run VMAF-based quality search if enabled in the encode config.
///
/// Parses `Trim()` commands from `obs_cut.avs`, converts frame
/// numbers to timestamps, and delegates to `dtvmgr_vmaf`.
///
/// Returns `None` when quality search is disabled or not configured.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn run_quality_search(
    encode: Option<&JlseEncode>,
    input: &Path,
    obs_cut_avs: &Path,
    ffmpeg_bin: &Path,
    cache_path: &Path,
    force: bool,
    total_stages: u8,
    on_progress: Option<&dyn Fn(ProgressEvent)>,
) -> Result<Option<dtvmgr_vmaf::SearchResult>> {
    let Some(qs) = encode
        .and_then(|e| e.quality_search.as_ref())
        .filter(|qs| qs.enabled)
    else {
        return Ok(None);
    };

    let encode_cfg = encode.context("encode config required for quality search")?;

    info!("running VMAF-based quality parameter search");

    // Parse Trim(start,end) from obs_cut.avs
    let avs_content = std::fs::read_to_string(obs_cut_avs)
        .with_context(|| format!("failed to read {}", obs_cut_avs.display()))?;
    let trims = output::chapter::parse_trims(&avs_content);

    // Convert frame pairs to ContentSegment (f64 seconds)
    let segments: Vec<dtvmgr_vmaf::ContentSegment> = trims
        .chunks_exact(2)
        .filter_map(|pair| {
            let (&start, &end) = (pair.first()?, pair.get(1)?);
            Some(dtvmgr_vmaf::ContentSegment {
                start_secs: output::chapter::frame_to_secs(start),
                end_secs: output::chapter::frame_to_secs(end),
            })
        })
        .collect();

    if segments.is_empty() {
        bail!("no content segments found in {}", obs_cut_avs.display());
    }

    // Build EncoderConfig from JlseEncode video settings
    let encoder = build_encoder_config(encode_cfg);

    // Build video filter with hwupload prepend when HW filter device is set
    let video_filter = build_vmaf_video_filter(encode_cfg);
    let hw_input_args = build_vmaf_hw_input_args(encode_cfg);
    let reference_filter = build_vmaf_reference_filter(encode_cfg, &video_filter);

    let sample_cfg = dtvmgr_vmaf::SampleConfig {
        duration_secs: qs.sample_duration_secs.unwrap_or(3.0),
        skip_secs: qs.skip_secs.unwrap_or(120.0),
        sample_every_secs: qs.sample_every_secs.unwrap_or(720.0),
        min_samples: qs.min_samples.unwrap_or(5),
        max_samples: qs.max_samples.unwrap_or(15),
        vmaf_subsample: qs.vmaf_subsample.unwrap_or(5),
    };

    let search_config = dtvmgr_vmaf::SearchConfig {
        ffmpeg_bin: ffmpeg_bin.to_owned(),
        input_file: input.to_owned(),
        content_segments: segments,
        encoder,
        video_filter,
        target_vmaf: qs.target_vmaf.unwrap_or(93.0),
        max_encoded_percent: qs.max_encoded_percent.unwrap_or(80.0),
        min_vmaf_tolerance: qs.min_vmaf_tolerance.unwrap_or(1.0),
        thorough: qs.thorough.unwrap_or(true),
        sample: sample_cfg,
        extra_encode_args: {
            let mut extra = encode_cfg
                .video
                .as_ref()
                .map(|v| v.extra.clone())
                .unwrap_or_default();
            crate::command::ffmpeg::add_stream_specifiers(&mut extra, "v");
            extra
        },
        extra_input_args: hw_input_args,
        reference_filter,
        temp_dir: None,
    };

    // Build cache config snapshot and try to load from cache
    let cache_cfg = build_cache_config(&search_config, &avs_content);
    if !force && let Some(cached) = load_quality_cache(cache_path, &cache_cfg) {
        info!(
            path = %cache_path.display(),
            "skipping quality search (use --force to re-run)"
        );
        return Ok(Some(cached));
    }

    // Emit stage start and bridge VMAF progress → pipeline progress
    let quality_stage: u8 = 4; // quality search is always stage 4
    if let Some(cb) = on_progress {
        cb(ProgressEvent::StageStart {
            stage: quality_stage,
            total: total_stages,
            name: String::from("quality search"),
        });
    }

    let last_vmaf = std::cell::Cell::new(0.0_f64);
    let vmaf_progress_cb = |evt: dtvmgr_vmaf::SearchProgress| {
        if let dtvmgr_vmaf::SearchProgress::IterationResult { vmaf, .. } = &evt {
            last_vmaf.set(f64::from(*vmaf));
        }
        if let Some(cb) = on_progress {
            let (pct, msg) = vmaf_progress_to_stage(&evt, last_vmaf.get());
            cb(ProgressEvent::StageProgress {
                percent: pct,
                log: msg.clone(),
            });
            cb(ProgressEvent::Log(msg));
        }
    };

    let result = dtvmgr_vmaf::find_optimal_quality(&search_config, Some(&vmaf_progress_cb))
        .context("VMAF quality search failed")?;

    info!(
        quality_param = %result.quality_param,
        quality_value = format!("{:.3}", result.quality_value),
        vmaf = format!("{:.3}", result.mean_vmaf),
        size_pct = format!("{:.1}", result.predicted_size_percent),
        iterations = result.iterations,
        "quality search completed"
    );

    // Cache the result for future runs
    save_quality_cache(cache_path, &cache_cfg, &result);

    Ok(Some(result))
}

/// Build a `dtvmgr_vmaf::EncoderConfig` from the TOML encode settings.
fn build_encoder_config(encode: &JlseEncode) -> dtvmgr_vmaf::EncoderConfig {
    let video = encode.video.as_ref();
    let codec = video.and_then(|v| v.codec.as_deref()).unwrap_or("libx264");

    let mut cfg = match codec {
        "av1_qsv" => dtvmgr_vmaf::EncoderConfig::av1_qsv(),
        "libsvtav1" => dtvmgr_vmaf::EncoderConfig::libsvtav1(),
        "h264_qsv" => dtvmgr_vmaf::EncoderConfig::h264_qsv(),
        "hevc_qsv" => dtvmgr_vmaf::EncoderConfig::hevc_qsv(),
        "libx265" => dtvmgr_vmaf::EncoderConfig::libx265(),
        _ => {
            let mut c = dtvmgr_vmaf::EncoderConfig::libx264();
            c.codec = String::from(codec);
            c
        }
    };

    // Override preset / pix_fmt from TOML if specified
    if let Some(preset) = video.and_then(|v| v.preset.clone()) {
        cfg.preset = Some(preset);
    }
    if let Some(pix_fmt) = video.and_then(|v| v.pix_fmt.clone()) {
        cfg.pix_fmt = Some(pix_fmt);
    }

    cfg
}

/// Build HW device init args from [`JlseEncode`] for VMAF search.
///
/// Extracts `-init_hw_device` and `-filter_hw_device` values so that
/// QSV VPP filters work during sample encoding and reference creation.
/// Returns an empty `Vec` when no HW filter device is configured.
fn build_vmaf_hw_input_args(encode: &JlseEncode) -> Vec<String> {
    let Some(input) = encode.input.as_ref() else {
        return Vec::new();
    };
    let mut args = Vec::new();
    if let Some(ref hw) = input.init_hw_device {
        args.push("-init_hw_device".to_owned());
        args.push(hw.clone());
    }
    if let Some(ref hw) = input.filter_hw_device {
        args.push("-filter_hw_device".to_owned());
        args.push(hw.clone());
    }
    args
}

/// Build the video filter string for VMAF sample encoding.
///
/// Delegates to [`prepare_hw_filter`](super::command::ffmpeg::prepare_hw_filter)
/// when `filter_hw_device` is set, which handles `hwupload`
/// prepend and `format={pix_fmt}` injection into VPP segments.
fn build_vmaf_video_filter(encode: &JlseEncode) -> String {
    let raw_filter = encode
        .video
        .as_ref()
        .and_then(|v| v.filter.clone())
        .unwrap_or_default();

    if raw_filter.is_empty() {
        return raw_filter;
    }

    let needs_hw = encode
        .input
        .as_ref()
        .and_then(|i| i.filter_hw_device.as_ref())
        .is_some();

    if needs_hw {
        let pix_fmt = encode.video.as_ref().and_then(|v| v.pix_fmt.as_deref());
        super::command::ffmpeg::prepare_hw_filter(&raw_filter, pix_fmt)
    } else {
        raw_filter
    }
}

/// Build the reference filter for FFV1 lossless reference creation.
///
/// FFV1 is a CPU-only encoder, so when VPP filters produce QSV
/// surface frames, `hwdownload` must be appended to transfer
/// frames back to system memory.  After `hwdownload`, an explicit
/// `format=` is required so that ffmpeg can negotiate a pixel
/// format that the software encoder (FFV1) accepts.  When
/// `pix_fmt` is configured (e.g. `p010le`), that value is used;
/// otherwise `nv12` is used as a safe default.
///
/// Returns `None` when no HW filter device is configured (the
/// caller should fall back to `video_filter`).
fn build_vmaf_reference_filter(encode: &JlseEncode, video_filter: &str) -> Option<String> {
    let has_hw_filter = encode
        .input
        .as_ref()
        .and_then(|i| i.filter_hw_device.as_ref())
        .is_some();

    if !has_hw_filter || video_filter.is_empty() {
        return None;
    }

    let fmt = encode
        .video
        .as_ref()
        .and_then(|v| v.pix_fmt.as_deref())
        .unwrap_or("nv12");

    Some(format!("{video_filter},hwdownload,format={fmt}"))
}

/// Emit a structured summary log with all pipeline metrics.
///
/// Called on both success and failure so that a single log search
/// (`pipeline summary`) surfaces all encode results.
///
/// Produces nested JSON fields (`input.*`, `output.*`) when exported
/// via `OTel` so that o2 displays a clean hierarchical structure.
#[allow(clippy::cast_precision_loss, clippy::as_conversions)]
fn emit_pipeline_summary(
    input_path: &Path,
    output_encode_args: &[String],
    summary: &PipelineSummary,
    status: &str,
) {
    let input_size_mb = std::fs::metadata(input_path)
        .ok()
        .map(|m| m.len() as f64 / 1_048_576.0);

    let ts_min = summary.ts_duration_secs.map(|s| s / 60.0);

    let output_size_mb = summary.output_size.map(|s| s as f64 / 1_048_576.0);
    let output_size_pct = match (summary.output_size, input_size_mb) {
        (Some(out), Some(in_mb)) if in_mb > 0.0 => Some(out as f64 / 1_048_576.0 / in_mb * 100.0),
        _ => None,
    };

    let output_file = summary.output.as_deref().map(|p| p.display().to_string());
    let encode_args_str = output_encode_args.join(" ");

    info!(
        status,
        input.file = %input_path.display(),
        input.size_mb = ?input_size_mb.map(round1),
        input.ts_secs = ?summary.ts_duration_secs.map(round1),
        input.ts_min = ?ts_min.map(round1),
        output.file = ?output_file,
        output.codec = summary.codec.as_deref().unwrap_or("unknown"),
        output.ffmpeg_args = %encode_args_str,
        output.quality_param = ?summary.quality_param,
        output.quality_value = ?summary.quality_value.map(round3),
        output.vmaf = ?summary.vmaf.map(round3),
        output.vmaf_subsample = ?summary.vmaf_subsample,
        output.size_mb = ?output_size_mb.map(round1),
        output.size_pct = ?output_size_pct.map(round1),
        output.avs_secs = ?summary.avs_duration_secs.map(round1),
        output.ratio_pct = ?summary.ratio_percent.map(round1),
        output.post_video_secs = ?summary.post_video_secs.map(round1),
        output.post_audio_secs = ?summary.post_audio_secs.map(round1),
        "pipeline summary"
    );
}

/// Round f64 to 1 decimal place for display.
fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Round f32 to 3 decimal places for display.
#[allow(clippy::arithmetic_side_effects)]
fn round3(v: f32) -> f64 {
    (f64::from(v) * 1000.0).round() / 1000.0
}

/// Convert a VMAF search progress event to a stage percent and log message.
///
/// Layout: sample extraction = 0–10%, iterations = 10–100%.
/// Assumes ~6 iterations for the progress bar estimate.
#[allow(clippy::arithmetic_side_effects)]
fn vmaf_progress_to_stage(evt: &dtvmgr_vmaf::SearchProgress, last_vmaf: f64) -> (f64, String) {
    const EST_ITERS: f64 = 6.0;

    match *evt {
        dtvmgr_vmaf::SearchProgress::SampleExtract { current, total } => {
            let pct = f64::from(current) / f64::from(total.max(1)) * 0.1;
            (pct, format!("extracting samples ({current}/{total})"))
        }
        dtvmgr_vmaf::SearchProgress::Encoding {
            iteration,
            quality,
            sample,
            total,
        } => {
            // Each sample has 2 phases (encode + score). Use a single
            // event index across both to guarantee monotonic progress.
            let steps = f64::from(total.max(1)) * 2.0;
            let event_idx = (f64::from(sample) - 1.0) * 2.0; // encode = even
            let iter_frac = f64::from(iteration - 1) / EST_ITERS;
            let pct = (iter_frac + event_idx / steps / EST_ITERS)
                .mul_add(0.9, 0.1)
                .clamp(0.0, 1.0);
            (
                pct,
                format!(
                    "encoding  ({sample:>2}/{total}) iter {iteration} q={quality:.3} vmaf={last_vmaf:06.3}"
                ),
            )
        }
        dtvmgr_vmaf::SearchProgress::Scoring {
            iteration,
            quality,
            sample,
            total,
        } => {
            let steps = f64::from(total.max(1)) * 2.0;
            let event_idx = (f64::from(sample) - 1.0).mul_add(2.0, 1.0); // score = odd
            let iter_frac = f64::from(iteration - 1) / EST_ITERS;
            let pct = (iter_frac + event_idx / steps / EST_ITERS)
                .mul_add(0.9, 0.1)
                .clamp(0.0, 1.0);
            (
                pct,
                format!(
                    "scoring   ({sample:>2}/{total}) iter {iteration} q={quality:.3} vmaf={last_vmaf:06.3}"
                ),
            )
        }
        dtvmgr_vmaf::SearchProgress::IterationResult {
            iteration,
            quality,
            vmaf,
            size_percent,
        } => {
            let pct = (f64::from(iteration) / EST_ITERS)
                .mul_add(0.9, 0.1)
                .clamp(0.0, 1.0);
            (
                pct,
                format!("iter {iteration}: q={quality:.3} vmaf={vmaf:.3} size={size_percent:.1}%"),
            )
        }
    }
}

/// Replace a quality token in an `ENCODER_SETTINGS` summary string.
///
/// Searches for a ` / `‐delimited token whose bare flag matches `param_name`
/// (e.g. `"crf"` matches `"crf:v 23"`), replaces the value portion with
/// `new_value`, and returns the modified string.
///
/// Each token has the format `flag value` where flag may include a stream
/// specifier (e.g. `crf:v`).  The flag portion is preserved so that stream
/// specifiers are not lost.
///
/// Returns the original string unchanged when no matching token is found.
fn replace_quality_token(summary: &str, param_name: &str, new_value: f32) -> String {
    let parts: Vec<&str> = summary.split(" / ").collect();
    let mut replaced = false;
    let updated: Vec<String> = parts
        .iter()
        .map(|p| {
            // Token format: "flag value" — split on first space.
            let flag_part = p.split(' ').next().unwrap_or(p);
            let bare = flag_part.split(':').next().unwrap_or(flag_part);
            if bare == param_name {
                replaced = true;
                format!("{flag_part} {new_value}")
            } else {
                (*p).to_owned()
            }
        })
        .collect();
    if replaced {
        updated.join(" / ")
    } else {
        summary.to_owned()
    }
}

/// Insert `token` right after the first quality-param token in a ` / `‐delimited
/// settings string.  Each token has the format `flag value` (e.g. `crf:v 23`).
/// The bare flag name (before any `:` specifier) is checked against known
/// quality flags (`crf`, `qp`, `global_quality`, `q`, `cq`).
///
/// If no quality token is found the token is inserted after the first element.
fn insert_token_after_quality(settings: &str, token: &str) -> String {
    let parts: Vec<&str> = settings.split(" / ").collect();
    let mut result: Vec<&str> = Vec::with_capacity(parts.len().saturating_add(1));
    let mut inserted = false;
    for part in &parts {
        result.push(part);
        if !inserted {
            // Extract the flag portion (before the space):
            //   "crf:v 23"          → flag_part "crf:v" → bare "crf"
            //   "global_quality 27"  → flag_part "global_quality" → bare "global_quality"
            let flag_part = part.split(' ').next().unwrap_or(part);
            let bare = flag_part.split(':').next().unwrap_or(flag_part);
            let flag = format!("-{bare}");
            if dtvmgr_vmaf::QualityParam::ALL_FLAGS.contains(&flag.as_str()) {
                result.push(token);
                inserted = true;
            }
        }
    }
    if !inserted && !result.is_empty() {
        result.insert(1.min(result.len()), token);
    }
    result.join(" / ")
}

/// Inject the quality search result into the output encode args.
///
/// Replaces any existing `-crf`, `-global_quality`, `-qp`, `-q`, or `-cq`
/// value, or appends the quality parameter if not present.
fn inject_quality_override(output_args: &mut Vec<String>, result: &dtvmgr_vmaf::SearchResult) {
    let value_str = format!("{}", result.quality_value);
    let flag_with_spec = format!("{}:v", result.quality_param);

    // After `add_stream_specifiers`, all video flags already carry `:v`.
    // Search only for `flag:v` patterns.
    let replaced = output_args.iter().position(|arg| {
        dtvmgr_vmaf::QualityParam::ALL_FLAGS_V
            .iter()
            .any(|&flag| arg == flag)
    });

    #[allow(clippy::indexing_slicing)]
    if let Some(idx) = replaced {
        output_args[idx].clone_from(&flag_with_spec);
        let val_idx = idx.saturating_add(1);
        let next_is_value = output_args
            .get(val_idx)
            .is_some_and(|s| !s.starts_with('-'));
        if next_is_value {
            output_args[val_idx].clone_from(&value_str);
        } else {
            output_args.insert(val_idx, value_str);
        }
    } else {
        output_args.push(flag_with_spec);
        output_args.push(value_str);
    }

    info!(
        param = %result.quality_param,
        value = %result.quality_value,
        "injected quality override into encode args"
    );
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]

    use crate::command::test_utils;

    use super::*;

    // ── validate_input ───────────────────────────────────────

    #[test]
    fn test_validate_input_ts() {
        assert!(validate_input(Path::new("/path/to/recording.ts")).is_ok());
    }

    #[test]
    fn test_validate_input_m2ts() {
        assert!(validate_input(Path::new("/path/to/recording.m2ts")).is_ok());
    }

    #[test]
    fn test_validate_input_mp4_rejected() {
        assert!(validate_input(Path::new("/path/to/video.mp4")).is_err());
    }

    #[test]
    fn test_validate_input_no_extension_rejected() {
        assert!(validate_input(Path::new("/path/to/noext")).is_err());
    }

    #[test]
    fn test_validate_input_empty_path_rejected() {
        assert!(validate_input(Path::new("")).is_err());
    }

    // ── write_obs_param ──────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_write_obs_param_with_channel() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("obs_param.txt");
        let channel = Channel {
            recognize: "NHK".to_owned(),
            install: String::new(),
            short: "NHK-G".to_owned(),
            service_id: "101".to_owned(),
        };
        let param = DetectionParam {
            jl_run: "JL_NHK.txt".to_owned(),
            flags: "fLOff".to_owned(),
            options: String::new(),
        };

        // Act
        write_obs_param(&path, Some(&channel), &param).unwrap();

        // Assert
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("channel_recognize=NHK"));
        assert!(content.contains("channel_short=NHK-G"));
        assert!(content.contains("channel_service_id=101"));
        assert!(content.contains("jl_run=JL_NHK.txt"));
        assert!(content.contains("flags=fLOff"));
        assert!(content.contains("options="));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_write_obs_param_without_channel() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("obs_param.txt");
        let param = DetectionParam::default();

        // Act
        write_obs_param(&path, None, &param).unwrap();

        // Assert
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("channel_recognize=\n"));
        assert!(content.contains("channel_short=\n"));
        assert!(content.contains("channel_service_id=\n"));
    }

    // ── select_avs_file ─────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_select_avs_file_cutcm() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::settings::init_output_paths(tmp.path(), "test").unwrap();

        // Act / Assert
        assert_eq!(
            select_avs_file(&paths, AvsTarget::CutCm),
            &paths.output_avs_in_cut
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_select_avs_file_cutcm_logo() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::settings::init_output_paths(tmp.path(), "test").unwrap();

        // Act / Assert
        assert_eq!(
            select_avs_file(&paths, AvsTarget::CutCmLogo),
            &paths.output_avs_in_cut_logo
        );
    }

    // ── resolve_output_path ─────────────────────────────────

    #[test]
    fn test_resolve_output_path_default() {
        // Arrange
        let input = Path::new("/rec/recording.ts");

        // Act
        let result = resolve_output_path(input, None, None, "mkv");

        // Assert
        assert_eq!(result, PathBuf::from("/rec/recording.mkv"));
    }

    #[test]
    fn test_resolve_output_path_with_outdir() {
        // Arrange
        let input = Path::new("/rec/recording.ts");

        // Act
        let result = resolve_output_path(input, Some(Path::new("/enc")), None, "mkv");

        // Assert
        assert_eq!(result, PathBuf::from("/enc/recording.mkv"));
    }

    #[test]
    fn test_resolve_output_path_with_outname() {
        // Arrange
        let input = Path::new("/rec/recording.ts");

        // Act
        let result = resolve_output_path(input, None, Some("custom"), "mkv");

        // Assert
        assert_eq!(result, PathBuf::from("/rec/custom.mkv"));
    }

    #[test]
    fn test_resolve_output_path_with_outdir_and_outname() {
        // Arrange
        let input = Path::new("/rec/recording.ts");

        // Act
        let result = resolve_output_path(input, Some(Path::new("/enc")), Some("custom"), "mkv");

        // Assert
        assert_eq!(result, PathBuf::from("/enc/custom.mkv"));
    }

    #[test]
    fn test_resolve_output_path_custom_extension() {
        // Arrange
        let input = Path::new("/rec/recording.ts");

        // Act
        let result = resolve_output_path(input, None, None, "mp4");

        // Assert
        assert_eq!(result, PathBuf::from("/rec/recording.mp4"));
    }

    // ── canonicalize_dirs ────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_canonicalize_dirs_success() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let jl = tmp.path().join("jl");
        let logo = tmp.path().join("logo");
        let result_dir = tmp.path().join("result");
        std::fs::create_dir_all(&jl).unwrap();
        std::fs::create_dir_all(&logo).unwrap();
        std::fs::create_dir_all(&result_dir).unwrap();

        let dirs = JlseDirs {
            jl,
            logo,
            result: result_dir,
        };

        // Act
        let canon = canonicalize_dirs(&dirs).unwrap();

        // Assert: canonicalized paths should be absolute
        assert!(canon.jl.is_absolute());
        assert!(canon.logo.is_absolute());
        assert!(canon.result.is_absolute());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_canonicalize_dirs_nonexistent() {
        // Arrange
        let dirs = JlseDirs {
            jl: PathBuf::from("/nonexistent/jl"),
            logo: PathBuf::from("/nonexistent/logo"),
            result: PathBuf::from("/nonexistent/result"),
        };

        // Act
        let result = canonicalize_dirs(&dirs);

        // Assert
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("failed to canonicalize"),
            "expected 'failed to canonicalize' in: {err}"
        );
    }

    // ── validate_encode_config ───────────────────────────────

    /// Helper to build a minimal `JlseEncode` with `quality_search` settings.
    fn encode_with_qs(enabled: bool, extra: Vec<String>) -> JlseEncode {
        JlseEncode {
            format: Some("mkv".to_owned()),
            input: None,
            video: Some(crate::types::EncodeVideo {
                codec: Some("libx264".to_owned()),
                preset: None,
                profile: None,
                pix_fmt: None,
                aspect: None,
                filter: None,
                extra,
            }),
            audio: None,
            duration_check: None,
            quality_search: Some(crate::types::QualitySearchConfig {
                enabled,
                target_vmaf: None,
                max_encoded_percent: None,
                min_vmaf_tolerance: None,
                thorough: None,
                sample_duration_secs: None,
                skip_secs: None,
                sample_every_secs: None,
                min_samples: None,
                max_samples: None,
                vmaf_subsample: None,
            }),
        }
    }

    #[test]
    fn test_validate_encode_config_qs_disabled_ok() {
        // Arrange
        let enc = encode_with_qs(false, vec!["-crf".to_owned(), "23".to_owned()]);

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_encode_config_none_ok() {
        // Arrange / Act
        let result = validate_encode_config(None);

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_encode_config_qs_enabled_no_extra_ok() {
        // Arrange
        let enc = encode_with_qs(true, Vec::new());

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_encode_config_qs_enabled_crf_conflict() {
        // Arrange
        let enc = encode_with_qs(true, vec!["-crf".to_owned(), "23".to_owned()]);

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("-crf"), "expected '-crf' in error: {msg}");
    }

    #[test]
    fn test_validate_encode_config_qs_enabled_global_quality_conflict() {
        // Arrange
        let enc = encode_with_qs(true, vec!["-global_quality".to_owned(), "25".to_owned()]);

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("-global_quality"),
            "expected '-global_quality' in error: {msg}"
        );
    }

    #[test]
    fn test_validate_encode_config_qs_enabled_non_quality_extra_ok() {
        // Arrange — only non-quality flags
        let enc = encode_with_qs(true, vec!["-color_range".to_owned(), "tv".to_owned()]);

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_encode_config_vpp_format_rejected() {
        // Arrange — VPP filter with redundant :format=
        let enc = JlseEncode {
            video: Some(crate::types::EncodeVideo {
                filter: Some("vpp_qsv=deinterlace=advanced:format=p010le:height=720".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("redundant"),
            "error should mention redundancy"
        );
    }

    #[test]
    fn test_validate_encode_config_vpp_without_format_ok() {
        // Arrange — VPP filter without :format= (correct usage)
        let enc = JlseEncode {
            video: Some(crate::types::EncodeVideo {
                filter: Some("vpp_qsv=deinterlace=advanced:height=720".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert
        assert!(result.is_ok());
    }

    // ── inject_quality_override ──────────────────────────────

    fn make_search_result(param: &str, value: f32) -> dtvmgr_vmaf::SearchResult {
        dtvmgr_vmaf::SearchResult {
            quality_value: value,
            quality_param: String::from(param),
            mean_vmaf: 93.5,
            predicted_size_percent: 45.0,
            iterations: 5,
        }
    }

    #[test]
    fn test_inject_quality_override_replace_crf() {
        // Arrange — args are already normalised with :v
        let mut args = vec![
            "-preset:v".to_owned(),
            "medium".to_owned(),
            "-crf:v".to_owned(),
            "23".to_owned(),
            "-color_range:v".to_owned(),
            "tv".to_owned(),
        ];
        let result = make_search_result("-crf", 18.0);

        // Act
        inject_quality_override(&mut args, &result);

        // Assert
        assert_eq!(args[2], "-crf:v");
        assert_eq!(args[3], "18");
    }

    #[test]
    fn test_inject_quality_override_replace_global_quality() {
        // Arrange — normalised output from to_output_args
        let mut args = vec!["-global_quality:v".to_owned(), "27".to_owned()];
        let result = make_search_result("-global_quality", 22.0);

        // Act
        inject_quality_override(&mut args, &result);

        // Assert
        assert_eq!(args[0], "-global_quality:v");
        assert_eq!(args[1], "22");
    }

    #[test]
    fn test_inject_quality_override_append_when_missing() {
        // Arrange
        let mut args = vec!["-preset".to_owned(), "slow".to_owned()];
        let result = make_search_result("-crf", 20.0);

        // Act
        inject_quality_override(&mut args, &result);

        // Assert — appended with `:v` specifier
        assert_eq!(args.len(), 4);
        assert_eq!(args[2], "-crf:v");
        assert_eq!(args[3], "20");
    }

    #[test]
    fn test_inject_quality_override_append_global_quality_when_missing() {
        // Arrange — no quality param in args at all
        let mut args = vec!["-preset".to_owned(), "slow".to_owned()];
        let result = make_search_result("-global_quality", 30.0);

        // Act
        inject_quality_override(&mut args, &result);

        // Assert — appended with `:v` to avoid libopus exit 234
        assert_eq!(args.len(), 4);
        assert_eq!(args[2], "-global_quality:v");
        assert_eq!(args[3], "30");
    }

    #[test]
    fn test_inject_quality_override_empty_args() {
        // Arrange
        let mut args: Vec<String> = Vec::new();
        let result = make_search_result("-crf", 25.0);

        // Act
        inject_quality_override(&mut args, &result);

        // Assert — appended with `:v` specifier
        assert_eq!(args, vec!["-crf:v", "25"]);
    }

    // ── vmaf_progress_to_stage ───────────────────────────────

    #[test]
    fn test_vmaf_progress_sample_extract_range() {
        // Arrange
        let evt = dtvmgr_vmaf::SearchProgress::SampleExtract {
            current: 3,
            total: 10,
        };

        // Act
        let (pct, _msg) = vmaf_progress_to_stage(&evt, 0.0);

        // Assert — sample extraction maps to 0–10%
        assert!(pct >= 0.0, "pct={pct} should be >= 0.0");
        assert!(pct <= 0.1, "pct={pct} should be <= 0.1");
    }

    #[test]
    fn test_vmaf_progress_encoding_shows_last_vmaf() {
        // Arrange — iter 2 encoding with previous VMAF of 93.5
        let evt = dtvmgr_vmaf::SearchProgress::Encoding {
            iteration: 2,
            quality: 22.0,
            sample: 1,
            total: 5,
        };

        // Act
        let (_pct, msg) = vmaf_progress_to_stage(&evt, 93.5);

        // Assert — message includes the previous iteration's VMAF
        assert!(
            msg.contains("vmaf=93.500"),
            "expected vmaf=93.500 in msg: {msg}"
        );
    }

    #[test]
    fn test_vmaf_progress_encoding_monotonic() {
        // Arrange — same iteration, sample 1 then sample 2
        let evt1 = dtvmgr_vmaf::SearchProgress::Encoding {
            iteration: 1,
            quality: 23.0,
            sample: 1,
            total: 5,
        };
        let evt2 = dtvmgr_vmaf::SearchProgress::Encoding {
            iteration: 1,
            quality: 23.0,
            sample: 2,
            total: 5,
        };

        // Act
        let (pct1, _) = vmaf_progress_to_stage(&evt1, 0.0);
        let (pct2, _) = vmaf_progress_to_stage(&evt2, 0.0);

        // Assert — later sample should have higher progress
        assert!(
            pct2 > pct1,
            "sample 2 ({pct2}) should be greater than sample 1 ({pct1})"
        );
    }

    #[test]
    fn test_vmaf_progress_scoring_higher_than_encoding() {
        // Arrange — same iteration and sample
        let enc = dtvmgr_vmaf::SearchProgress::Encoding {
            iteration: 1,
            quality: 23.0,
            sample: 3,
            total: 5,
        };
        let score = dtvmgr_vmaf::SearchProgress::Scoring {
            iteration: 1,
            quality: 23.0,
            sample: 3,
            total: 5,
        };

        // Act
        let (pct_enc, _) = vmaf_progress_to_stage(&enc, 0.0);
        let (pct_score, _) = vmaf_progress_to_stage(&score, 0.0);

        // Assert — scoring same sample should be higher
        assert!(
            pct_score > pct_enc,
            "scoring ({pct_score}) should be > encoding ({pct_enc})"
        );
    }

    #[test]
    fn test_vmaf_progress_iteration_result() {
        // Arrange
        let evt = dtvmgr_vmaf::SearchProgress::IterationResult {
            iteration: 3,
            quality: 20.0,
            vmaf: 94.5,
            size_percent: 42.0,
        };

        // Act
        let (pct, msg) = vmaf_progress_to_stage(&evt, 0.0);

        // Assert — percent based on iteration / EST_ITERS
        assert!(pct >= 0.1, "pct={pct} should be >= 0.1 (past extraction)");
        assert!(pct <= 1.0, "pct={pct} should be <= 1.0");
        assert!(msg.contains("iter 3"), "msg should contain 'iter 3': {msg}");
    }

    // ── build_encoder_config ─────────────────────────────────

    fn encode_with_codec(codec: &str) -> JlseEncode {
        JlseEncode {
            video: Some(crate::types::EncodeVideo {
                codec: Some(codec.to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_build_encoder_config_libx264() {
        // Arrange
        let enc = encode_with_codec("libx264");

        // Act
        let cfg = build_encoder_config(&enc);

        // Assert
        assert_eq!(cfg.codec, "libx264");
        assert_eq!(cfg.quality_param, dtvmgr_vmaf::QualityParam::Crf);
    }

    #[test]
    fn test_build_encoder_config_av1_qsv() {
        // Arrange
        let enc = encode_with_codec("av1_qsv");

        // Act
        let cfg = build_encoder_config(&enc);

        // Assert
        assert_eq!(cfg.codec, "av1_qsv");
        assert_eq!(cfg.quality_param, dtvmgr_vmaf::QualityParam::GlobalQuality);
    }

    #[test]
    fn test_build_encoder_config_h264_qsv() {
        // Arrange
        let enc = encode_with_codec("h264_qsv");

        // Act
        let cfg = build_encoder_config(&enc);

        // Assert
        assert_eq!(cfg.codec, "h264_qsv");
        assert_eq!(cfg.quality_param, dtvmgr_vmaf::QualityParam::GlobalQuality);
    }

    #[test]
    fn test_build_encoder_config_hevc_qsv() {
        // Arrange
        let enc = encode_with_codec("hevc_qsv");

        // Act
        let cfg = build_encoder_config(&enc);

        // Assert
        assert_eq!(cfg.codec, "hevc_qsv");
        assert_eq!(cfg.quality_param, dtvmgr_vmaf::QualityParam::GlobalQuality);
    }

    #[test]
    fn test_build_encoder_config_unknown_codec_fallback() {
        // Arrange — unknown codec falls back to libx264 preset but with overridden name
        let enc = encode_with_codec("hevc_nvenc");

        // Act
        let cfg = build_encoder_config(&enc);

        // Assert
        assert_eq!(cfg.codec, "hevc_nvenc");
        assert_eq!(cfg.quality_param, dtvmgr_vmaf::QualityParam::Crf);
    }

    #[test]
    fn test_build_encoder_config_libsvtav1() {
        // Arrange
        let enc = encode_with_codec("libsvtav1");

        // Act
        let cfg = build_encoder_config(&enc);

        // Assert
        assert_eq!(cfg.codec, "libsvtav1");
        assert_eq!(cfg.quality_param, dtvmgr_vmaf::QualityParam::Crf);
    }

    #[test]
    fn test_build_encoder_config_libx265() {
        // Arrange
        let enc = encode_with_codec("libx265");

        // Act
        let cfg = build_encoder_config(&enc);

        // Assert
        assert_eq!(cfg.codec, "libx265");
        assert_eq!(cfg.quality_param, dtvmgr_vmaf::QualityParam::Crf);
    }

    #[test]
    fn test_build_encoder_config_no_video_section() {
        // Arrange — encode config with no video section
        let enc = JlseEncode {
            video: None,
            ..Default::default()
        };

        // Act
        let cfg = build_encoder_config(&enc);

        // Assert — defaults to libx264
        assert_eq!(cfg.codec, "libx264");
        assert_eq!(cfg.quality_param, dtvmgr_vmaf::QualityParam::Crf);
    }

    #[test]
    fn test_build_encoder_config_no_codec() {
        // Arrange — video section with no codec
        let enc = JlseEncode {
            video: Some(crate::types::EncodeVideo {
                codec: None,
                ..Default::default()
            }),
            ..Default::default()
        };

        // Act
        let cfg = build_encoder_config(&enc);

        // Assert — defaults to libx264
        assert_eq!(cfg.codec, "libx264");
    }

    #[test]
    fn test_build_encoder_config_toml_overrides_preset() {
        // Arrange
        let enc = JlseEncode {
            video: Some(crate::types::EncodeVideo {
                codec: Some("av1_qsv".to_owned()),
                preset: Some("veryslow".to_owned()),
                pix_fmt: Some("yuv420p10le".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Act
        let cfg = build_encoder_config(&enc);

        // Assert — TOML values override encoder defaults
        assert_eq!(cfg.preset.as_deref(), Some("veryslow"));
        assert_eq!(cfg.pix_fmt.as_deref(), Some("yuv420p10le"));
    }

    // ── build_vmaf_hw_input_args ──────────────────────────────

    #[test]
    fn test_build_vmaf_hw_input_args_with_hw_device() {
        // Arrange
        let enc = JlseEncode {
            input: Some(crate::types::EncodeInput {
                init_hw_device: Some("qsv=hw".to_owned()),
                filter_hw_device: Some("hw".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Act
        let args = build_vmaf_hw_input_args(&enc);

        // Assert
        assert_eq!(
            args,
            vec!["-init_hw_device", "qsv=hw", "-filter_hw_device", "hw"]
        );
    }

    #[test]
    fn test_build_vmaf_hw_input_args_no_hw() {
        // Arrange
        let enc = JlseEncode::default();

        // Act
        let args = build_vmaf_hw_input_args(&enc);

        // Assert
        assert!(args.is_empty());
    }

    #[test]
    fn test_build_vmaf_hw_input_args_only_init_hw_device() {
        // Arrange
        let enc = JlseEncode {
            input: Some(crate::types::EncodeInput {
                init_hw_device: Some("qsv=hw".to_owned()),
                filter_hw_device: None,
                ..Default::default()
            }),
            ..Default::default()
        };

        // Act
        let args = build_vmaf_hw_input_args(&enc);

        // Assert
        assert_eq!(args, vec!["-init_hw_device", "qsv=hw"]);
    }

    // ── build_vmaf_video_filter ─────────────────────────────

    #[test]
    fn test_build_vmaf_video_filter_sw_passthrough() {
        // Arrange — SW mode: no filter_hw_device
        let enc = JlseEncode {
            video: Some(crate::types::EncodeVideo {
                filter: Some(
                    "yadif=mode=send_frame:parity=auto:deint=all,scale=w=1280:h=720".to_owned(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Act
        let filter = build_vmaf_video_filter(&enc);

        // Assert — unchanged
        assert_eq!(
            filter,
            "yadif=mode=send_frame:parity=auto:deint=all,scale=w=1280:h=720"
        );
    }

    #[test]
    fn test_build_vmaf_video_filter_hwupload_auto_prepend_and_format_inject() {
        // Arrange — HW mode: filter_hw_device set, pix_fmt = p010le
        let enc = JlseEncode {
            input: Some(crate::types::EncodeInput {
                init_hw_device: Some("qsv=hw".to_owned()),
                filter_hw_device: Some("hw".to_owned()),
                ..Default::default()
            }),
            video: Some(crate::types::EncodeVideo {
                filter: Some("vpp_qsv=deinterlace=advanced:height=720:width=1280".to_owned()),
                pix_fmt: Some("p010le".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Act
        let filter = build_vmaf_video_filter(&enc);

        // Assert — format=nv12 + hwupload prepended, format=p010le injected
        assert_eq!(
            filter,
            "format=nv12,hwupload=extra_hw_frames=64,vpp_qsv=deinterlace=advanced:height=720:width=1280:format=p010le"
        );
    }

    #[test]
    fn test_build_vmaf_video_filter_hwupload_already_present() {
        // Arrange — user already included hwupload
        let enc = JlseEncode {
            input: Some(crate::types::EncodeInput {
                filter_hw_device: Some("hw".to_owned()),
                ..Default::default()
            }),
            video: Some(crate::types::EncodeVideo {
                filter: Some("hwupload=extra_hw_frames=32,vpp_qsv=framerate=30".to_owned()),
                pix_fmt: Some("p010le".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Act
        let filter = build_vmaf_video_filter(&enc);

        // Assert — no duplicate hwupload, format injected
        assert_eq!(
            filter,
            "hwupload=extra_hw_frames=32,vpp_qsv=framerate=30:format=p010le"
        );
    }

    #[test]
    fn test_build_vmaf_video_filter_default_sw_filter() {
        // Arrange — default config uses SW yadif+scale filter
        let enc = JlseEncode::default();

        // Act
        let filter = build_vmaf_video_filter(&enc);

        // Assert — default SW filter unchanged (no HW device)
        assert!(!filter.is_empty());
        assert!(filter.contains("yadif"), "expected default SW filter");
    }

    // ── build_vmaf_reference_filter ─────────────────────────

    #[test]
    fn test_build_vmaf_reference_filter_hw_mode() {
        // Arrange — video_filter already processed by build_vmaf_video_filter
        // (hwupload prepended, format= injected by prepare_hw_filter)
        let enc = JlseEncode {
            input: Some(crate::types::EncodeInput {
                filter_hw_device: Some("hw".to_owned()),
                ..Default::default()
            }),
            video: Some(crate::types::EncodeVideo {
                pix_fmt: Some("p010le".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let video_filter = "format=nv12,hwupload=extra_hw_frames=64,vpp_qsv=deinterlace=advanced:height=720:width=1280:format=p010le,setfield=mode=prog";

        // Act
        let ref_filter = build_vmaf_reference_filter(&enc, video_filter);

        // Assert — hwdownload + format= appended for FFV1
        assert_eq!(
            ref_filter.as_deref(),
            Some(
                "format=nv12,hwupload=extra_hw_frames=64,vpp_qsv=deinterlace=advanced:height=720:width=1280:format=p010le,setfield=mode=prog,hwdownload,format=p010le"
            )
        );
    }

    #[test]
    fn test_build_vmaf_reference_filter_sw_mode() {
        // Arrange — no HW
        let enc = JlseEncode::default();
        let video_filter = "yadif=mode=send_frame,scale=1280:720";

        // Act
        let ref_filter = build_vmaf_reference_filter(&enc, video_filter);

        // Assert — None, caller falls back to video_filter
        assert!(ref_filter.is_none());
    }

    #[test]
    fn test_build_vmaf_reference_filter_hw_no_pix_fmt() {
        // Arrange — HW mode, no pix_fmt — hwdownload auto-negotiates format
        let enc = JlseEncode {
            input: Some(crate::types::EncodeInput {
                filter_hw_device: Some("hw".to_owned()),
                ..Default::default()
            }),
            video: Some(crate::types::EncodeVideo {
                pix_fmt: None,
                ..Default::default()
            }),
            ..Default::default()
        };
        let video_filter = "hwupload=extra_hw_frames=64,vpp_qsv=framerate=30";

        // Act
        let ref_filter = build_vmaf_reference_filter(&enc, video_filter);

        // Assert — hwdownload with default nv12 format
        assert_eq!(
            ref_filter.as_deref(),
            Some("hwupload=extra_hw_frames=64,vpp_qsv=framerate=30,hwdownload,format=nv12")
        );
    }

    #[test]
    fn test_build_vmaf_reference_filter_empty_filter() {
        // Arrange — HW mode but empty filter
        let enc = JlseEncode {
            input: Some(crate::types::EncodeInput {
                filter_hw_device: Some("hw".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Act
        let ref_filter = build_vmaf_reference_filter(&enc, "");

        // Assert — empty filter → None
        assert!(ref_filter.is_none());
    }

    // ── round1 / round3 ─────────────────────────────────────

    #[test]
    fn test_round1_basic() {
        // Arrange / Act / Assert
        assert!((round1(3.456) - 3.5).abs() < f64::EPSILON);
        assert!((round1(3.449) - 3.4).abs() < f64::EPSILON);
        assert!((round1(0.0) - 0.0).abs() < f64::EPSILON);
        assert!((round1(-1.25) - (-1.3_f64)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_round3_basic() {
        // Arrange / Act / Assert
        assert!((round3(93.1234) - 93.123).abs() < 1e-12);
        assert!((round3(93.1236) - 93.124).abs() < 1e-12);
        assert!((round3(0.0) - 0.0).abs() < f64::EPSILON);
    }

    // ── Mock-based integration tests ─────────────────────────

    /// Create a mock `chapter_exe` that writes output to the file specified
    /// via `-o` flag (8th positional argument).
    fn write_mock_chapter_exe(dir: &Path) -> PathBuf {
        let script = "#!/bin/bash\n\
            # Args: -v <avs> -s 8 -e 4 -o <output>\n\
            echo 'mock chapter_exe output' > \"$8\"\n";
        test_utils::write_script(dir, "chapter_exe", script)
    }

    /// Create a mock logoframe that writes txt (`-oa`, arg 5) and avs
    /// (`-o`, arg 7) output files.
    fn write_mock_logoframe(dir: &Path) -> PathBuf {
        // Args: $1=avs $2=-logo $3=logo $4=-oa $5=txt $6=-o $7=avs_out
        let script = "#!/bin/bash\n\
            echo '0,100,1' > \"$5\"\n\
            echo '' > \"$7\"\n";
        test_utils::write_script(dir, "logoframe", script)
    }

    /// Create a mock `join_logo_scp` that parses `-o` and `-oscp` flags
    /// and writes `Trim()` and jlscp structure output.
    fn write_mock_join_logo_scp(dir: &Path) -> PathBuf {
        let script = r#"#!/bin/bash
OUT=""
OSCP=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        -o) OUT="$2"; shift 2;;
        -oscp) OSCP="$2"; shift 2;;
        *) shift;;
    esac
done
echo 'Trim(100,500)' > "$OUT"
echo 'Trim(600,900)' >> "$OUT"
printf '  100 500 13 -1 1 0.00:Ncut\n' > "$OSCP"
printf '  500 600 3 -1 1 0.00:CM\n' >> "$OSCP"
printf '  600 900 10 -1 1 0.00:Ncut\n' >> "$OSCP"
"#;
        test_utils::write_script(dir, "join_logo_scp", script)
    }

    /// Create a mock ffprobe that returns duration or frame rate depending
    /// on its arguments.
    fn write_mock_ffprobe(dir: &Path) -> PathBuf {
        let script = r#"#!/bin/bash
for arg in "$@"; do
    if [[ "$arg" == *"duration"* ]]; then
        echo "1440.0"
        exit 0
    fi
    if [[ "$arg" == *"r_frame_rate"* ]]; then
        echo "30000/1001"
        exit 0
    fi
    if [[ "$arg" == *"sample_rate"* ]]; then
        echo "48000"
        exit 0
    fi
    if [[ "$arg" == *"codec_type"* ]]; then
        echo "video"
        exit 0
    fi
done
echo "1440.0"
"#;
        test_utils::write_script(dir, "ffprobe", script)
    }

    /// Create a mock ffmpeg that writes a minimal file at the last argument.
    fn write_mock_ffmpeg(dir: &Path) -> PathBuf {
        let script = "#!/bin/bash\n\
            output=\"${@: -1}\"\n\
            echo 'mock' > \"$output\"\n";
        test_utils::write_script(dir, "ffmpeg", script)
    }

    /// Create a mock tstables that outputs minimal XML to stdout.
    fn write_mock_tstables(dir: &Path) -> PathBuf {
        let script = "#!/bin/bash\n\
            echo '<?xml version=\"1.0\"?><tsduck></tsduck>'\n";
        test_utils::write_script(dir, "tstables", script)
    }

    /// Set up the full directory structure and data files needed for
    /// a pipeline integration test.
    ///
    /// Returns `(tmp, input_ts, config)`.
    fn setup_pipeline_fixture(encode: bool) -> (tempfile::TempDir, PathBuf, JlseConfig) {
        let tmp = tempfile::TempDir::new().unwrap();
        let tmp_path = tmp.path();

        // Create mock binaries
        let bin_dir = tmp_path.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let chapter_exe = write_mock_chapter_exe(&bin_dir);
        let logoframe = write_mock_logoframe(&bin_dir);
        let join_logo_scp = write_mock_join_logo_scp(&bin_dir);
        let ffprobe = write_mock_ffprobe(&bin_dir);
        let ffmpeg = write_mock_ffmpeg(&bin_dir);
        let tstables = write_mock_tstables(&bin_dir);

        // Create fake input TS file
        let input_ts = tmp_path.join("input.ts");
        std::fs::write(&input_ts, "fake ts content").unwrap();

        // Create JL directory structure with data files
        let jl_dir = tmp_path.join("JL");
        let data_dir = jl_dir.join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // ChList.csv — header + one entry
        std::fs::write(
            data_dir.join("ChList.csv"),
            "recognize,install,short,service_id\nTEST,,TEST,101\n",
        )
        .unwrap();

        // ChParamJL1.csv / ChParamJL2.csv — header + one entry
        let param_csv = "channel,title,jl_run,flags,options,comment_view,comment\n\
             TEST,,JL_test.txt,,,,\n";
        std::fs::write(data_dir.join("ChParamJL1.csv"), param_csv).unwrap();
        std::fs::write(data_dir.join("ChParamJL2.csv"), param_csv).unwrap();

        // JL command file
        std::fs::write(jl_dir.join("JL_test.txt"), "# JL command\n").unwrap();

        // Create logo directory with a matching .lgd file
        let logo_dir = tmp_path.join("logo");
        std::fs::create_dir_all(&logo_dir).unwrap();
        std::fs::write(logo_dir.join("TEST.lgd"), "mock logo data").unwrap();

        // Create result directory
        let result_dir = tmp_path.join("result");
        std::fs::create_dir_all(&result_dir).unwrap();

        let encode_config = if encode {
            Some(JlseEncode::default())
        } else {
            None
        };

        let config = JlseConfig {
            dirs: JlseDirs {
                jl: jl_dir,
                logo: logo_dir,
                result: result_dir,
            },
            bins: crate::types::JlseBins {
                chapter_exe: Some(chapter_exe),
                logoframe: Some(logoframe),
                join_logo_scp: Some(join_logo_scp),
                ffprobe: Some(ffprobe),
                ffmpeg: Some(ffmpeg),
                tstables: Some(tstables),
            },
            encode: encode_config,
        };

        (tmp, input_ts, config)
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_pipeline_inner_cm_detection_only() {
        // Arrange — full mock pipeline without encoding
        let (_tmp, input_ts, config) = setup_pipeline_fixture(false);

        let ctx = PipelineContext {
            input: input_ts,
            channel_name: Some("TEST".to_owned()),
            config,
            encode: false,
            target: AvsTarget::CutCm,
            add_chapter: false,
            out_dir: None,
            out_name: None,
            out_extension: None,
            remove: false,
            progress_mode: None,
            skip_duration_check: true,
            force: true,
        };

        let mut summary = PipelineSummary::default();

        // Act
        let result = run_pipeline_inner(&ctx, None, &mut summary);

        // Assert — pipeline should complete successfully
        assert!(result.is_ok(), "pipeline failed: {:#}", result.unwrap_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_pipeline_inner_with_filter_generation() {
        // Arrange — test the ffmpeg filter generation path
        let (_tmp, input_ts, config) = setup_pipeline_fixture(false);

        let ctx = PipelineContext {
            input: input_ts,
            channel_name: Some("TEST".to_owned()),
            config,
            encode: false,
            target: AvsTarget::CutCm,
            add_chapter: false,
            out_dir: None,
            out_name: None,
            out_extension: None,
            remove: false,
            progress_mode: None,
            skip_duration_check: true,
            force: true,
        };

        let mut summary = PipelineSummary::default();

        // Act
        let result = run_pipeline_inner(&ctx, None, &mut summary);

        // Assert
        assert!(
            result.is_ok(),
            "pipeline with filter failed: {:#}",
            result.unwrap_err()
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_pipeline_inner_force_false_skips_cached() {
        // Arrange — run pipeline twice; second run should skip cached steps
        let (tmp, input_ts, config) = setup_pipeline_fixture(false);

        let ctx = PipelineContext {
            input: input_ts,
            channel_name: Some("TEST".to_owned()),
            config,
            encode: false,
            target: AvsTarget::CutCmLogo,
            add_chapter: false,
            out_dir: None,
            out_name: None,
            out_extension: None,
            remove: false,
            progress_mode: None,
            skip_duration_check: true,
            force: true,
        };

        let mut summary = PipelineSummary::default();

        // Act — first run populates output files
        let result1 = run_pipeline_inner(&ctx, None, &mut summary);
        assert!(
            result1.is_ok(),
            "first run failed: {:#}",
            result1.unwrap_err()
        );

        // Second run with force=false should skip cached outputs
        let ctx2 = PipelineContext {
            force: false,
            ..ctx
        };
        let mut summary2 = PipelineSummary::default();

        let result2 = run_pipeline_inner(&ctx2, None, &mut summary2);

        // Assert — should succeed (with skipped steps)
        assert!(
            result2.is_ok(),
            "second run (force=false) failed: {:#}",
            result2.unwrap_err()
        );

        // Verify intermediate files exist in result dir
        let result_dir = tmp.path().join("result").join("input");
        assert!(result_dir.join("obs_chapterexe.txt").exists());
        assert!(result_dir.join("obs_logoframe.txt").exists());
        assert!(result_dir.join("obs_cut.avs").exists());
        assert!(result_dir.join("obs_jlscp.txt").exists());
        assert!(result_dir.join("in_cutcm.avs").exists());
        assert!(result_dir.join("in_cutcm_logo.avs").exists());
        assert!(result_dir.join("obs_chapter_org.chapter.txt").exists());
        assert!(result_dir.join("obs_chapter_cut.chapter.txt").exists());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_pipeline_inner_with_remove() {
        // Arrange — test intermediate file cleanup
        let (tmp, input_ts, config) = setup_pipeline_fixture(false);

        let ctx = PipelineContext {
            input: input_ts,
            channel_name: Some("TEST".to_owned()),
            config,
            encode: false,
            target: AvsTarget::CutCm,
            add_chapter: false,
            out_dir: None,
            out_name: None,
            out_extension: None,
            remove: true,
            progress_mode: None,
            skip_duration_check: true,
            force: true,
        };

        let mut summary = PipelineSummary::default();

        // Act
        let result = run_pipeline_inner(&ctx, None, &mut summary);

        // Assert — pipeline succeeds and intermediate dir is removed
        assert!(
            result.is_ok(),
            "pipeline with remove failed: {:#}",
            result.unwrap_err()
        );
        let result_dir = tmp.path().join("result").join("input");
        assert!(
            !result_dir.exists(),
            "intermediate directory should be removed"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_pipeline_inner_no_channel_detected() {
        // Arrange — no channel_name override, so detection depends on filename/SID
        let (_tmp, input_ts, config) = setup_pipeline_fixture(false);

        let ctx = PipelineContext {
            input: input_ts,
            channel_name: None,
            config,
            encode: false,
            target: AvsTarget::CutCm,
            add_chapter: false,
            out_dir: None,
            out_name: None,
            out_extension: None,
            remove: false,
            progress_mode: None,
            skip_duration_check: true,
            force: true,
        };

        let mut summary = PipelineSummary::default();

        // Act — logoframe will fail because no channel detected (no logo file)
        let result = run_pipeline_inner(&ctx, None, &mut summary);

        // Assert — should fail at logoframe step (no channel => no logo)
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("no channel detected") || err.contains("logo"),
            "unexpected error: {err}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_pipeline_wrapper() {
        // Arrange — test run_pipeline() which wraps run_pipeline_inner()
        let (_tmp, input_ts, config) = setup_pipeline_fixture(false);

        let ctx = PipelineContext {
            input: input_ts,
            channel_name: Some("TEST".to_owned()),
            config,
            encode: false,
            target: AvsTarget::CutCm,
            add_chapter: false,
            out_dir: None,
            out_name: None,
            out_extension: None,
            remove: false,
            progress_mode: None,
            skip_duration_check: true,
            force: true,
        };

        // Act — test the public run_pipeline() wrapper
        let result = run_pipeline(&ctx, None);

        // Assert
        assert!(
            result.is_ok(),
            "run_pipeline failed: {:#}",
            result.unwrap_err()
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_pipeline_wrapper_failure_logs_summary() {
        // Arrange — invalid extension to trigger early failure
        let (_tmp, _input_ts, config) = setup_pipeline_fixture(false);
        let bad_input = PathBuf::from("/nonexistent/file.mp4");

        let ctx = PipelineContext {
            input: bad_input,
            channel_name: Some("TEST".to_owned()),
            config,
            encode: false,
            target: AvsTarget::CutCm,
            add_chapter: false,
            out_dir: None,
            out_name: None,
            out_extension: None,
            remove: false,
            progress_mode: None,
            skip_duration_check: true,
            force: true,
        };

        // Act — run_pipeline logs summary even on failure
        let result = run_pipeline(&ctx, None);

        // Assert
        assert!(result.is_err());
    }

    // ── emit_pipeline_summary ─────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_emit_pipeline_summary_completed() {
        // Arrange — summary with all fields populated
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("input.ts");
        std::fs::write(&input, "fake ts data").unwrap();

        let summary = PipelineSummary {
            output: Some(tmp.path().join("output.mkv")),
            codec: Some("av1_qsv".to_owned()),
            quality_value: Some(25.0),
            quality_param: Some("-global_quality".to_owned()),
            vmaf: Some(94.5),
            vmaf_subsample: Some(5),
            output_size: Some(500_000),
            ts_duration_secs: Some(1440.0),
            avs_duration_secs: Some(1200.0),
            ratio_percent: Some(83.3),
            post_video_secs: Some(1199.5),
            post_audio_secs: Some(1200.0),
        };
        let encode_args = vec!["-preset".to_owned(), "medium".to_owned()];

        // Act / Assert — should not panic
        emit_pipeline_summary(&input, &encode_args, &summary, "completed");
    }

    #[test]
    fn test_emit_pipeline_summary_failed_with_defaults() {
        // Arrange — summary with default (None) fields, non-existent input
        let summary = PipelineSummary::default();
        let input = Path::new("/nonexistent/input.ts");
        let encode_args: Vec<String> = Vec::new();

        // Act / Assert — should not panic even with missing file
        emit_pipeline_summary(input, &encode_args, &summary, "failed");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_emit_pipeline_summary_partial_fields() {
        // Arrange — some fields populated, some None
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("test.ts");
        std::fs::write(&input, "data").unwrap();

        let summary = PipelineSummary {
            output: None,
            codec: Some("libx264".to_owned()),
            quality_value: None,
            quality_param: None,
            vmaf: None,
            vmaf_subsample: None,
            output_size: None,
            ts_duration_secs: Some(600.0),
            avs_duration_secs: None,
            ratio_percent: None,
            post_video_secs: None,
            post_audio_secs: None,
        };
        let encode_args = vec!["-crf".to_owned(), "23".to_owned()];

        // Act / Assert
        emit_pipeline_summary(&input, &encode_args, &summary, "completed");
    }

    // ── inject_quality_override (edge: flag at last position) ─

    #[test]
    fn test_inject_quality_override_flag_at_end_no_value() {
        // Arrange — normalised quality flag at end with no following value
        let mut args = vec![
            "-preset:v".to_owned(),
            "slow".to_owned(),
            "-crf:v".to_owned(),
        ];
        let result = make_search_result("-crf", 19.0);

        // Act
        inject_quality_override(&mut args, &result);

        // Assert — value appended after existing flag
        assert_eq!(args[2], "-crf:v");
        assert_eq!(args[3], "19");
    }

    #[test]
    fn test_inject_quality_override_replace_qp() {
        // Arrange — normalised -qp:v flag
        let mut args = vec!["-qp:v".to_owned(), "30".to_owned()];
        let result = make_search_result("-qp", 25.0);

        // Act
        inject_quality_override(&mut args, &result);

        // Assert
        assert_eq!(args[0], "-qp:v");
        assert_eq!(args[1], "25");
    }

    // ── extract_pat_sids ────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_pat_sids_missing_binary() {
        // Arrange — non-existent binary
        let result = extract_pat_sids(Path::new("/nonexistent/tstables"), Path::new("/tmp/any.ts"));

        // Assert — gracefully returns None
        assert!(result.is_none());
    }

    // ── validate_encode_config (additional) ──────────────────

    #[test]
    fn test_validate_encode_config_qs_enabled_qp_conflict() {
        // Arrange
        let enc = encode_with_qs(true, vec!["-qp".to_owned(), "30".to_owned()]);

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("-qp"), "expected '-qp' in error: {msg}");
    }

    #[test]
    fn test_validate_encode_config_qs_enabled_crf_colon_v_conflict() {
        // Arrange — "-crf:v" should also be caught as conflicting
        let enc = encode_with_qs(true, vec!["-crf:v".to_owned(), "23".to_owned()]);

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert — "-crf:" prefix with flag "crf" starts_with check
        // The code checks: arg.starts_with(&format!("{flag}:"))
        // "-crf:v" starts with "-crf:" so it should conflict
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_encode_config_qs_no_video_section() {
        // Arrange — quality_search enabled but no video section
        let enc = JlseEncode {
            video: None,
            quality_search: Some(crate::types::QualitySearchConfig {
                enabled: true,
                target_vmaf: None,
                max_encoded_percent: None,
                min_vmaf_tolerance: None,
                thorough: None,
                sample_duration_secs: None,
                skip_secs: None,
                sample_every_secs: None,
                min_samples: None,
                max_samples: None,
                vmaf_subsample: None,
            }),
            ..Default::default()
        };

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert — no video.extra to conflict with, should be ok
        assert!(result.is_ok());
    }

    // ── resolve_output_path (edge case) ────────────────────

    #[test]
    fn test_resolve_output_path_no_parent_no_stem() {
        // Arrange — edge case: input with no parent or stem
        let input = Path::new("file.ts");

        // Act
        let result = resolve_output_path(input, None, None, "mkv");

        // Assert — should still produce a valid path
        assert_eq!(result, PathBuf::from("file.mkv"));
    }

    // ── generate_chapters ────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_generate_chapters_basic() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::settings::init_output_paths(tmp.path(), "test").unwrap();

        // Write required input files
        std::fs::write(&paths.output_avs_cut, "Trim(100,500)\nTrim(600,900)\n").unwrap();
        std::fs::write(
            &paths.jlscp_output,
            "  100 500 13 -1 1 0.00:Ncut\n  500 600 3 -1 1 0.00:CM\n  600 900 10 -1 1 0.00:Ncut\n",
        )
        .unwrap();

        // Act
        let result = generate_chapters(&paths);

        // Assert
        assert!(
            result.is_ok(),
            "generate_chapters failed: {:#}",
            result.unwrap_err()
        );
        assert!(paths.file_txt_cpt_org.exists());
        assert!(paths.file_txt_cpt_cut.exists());
    }

    // ── vmaf_progress_to_stage (additional) ──────────────────

    #[test]
    fn test_vmaf_progress_sample_extract_first() {
        // Arrange — first sample
        let evt = dtvmgr_vmaf::SearchProgress::SampleExtract {
            current: 1,
            total: 10,
        };

        // Act
        let (pct, msg) = vmaf_progress_to_stage(&evt, 0.0);

        // Assert
        assert!((0.0..=0.1).contains(&pct));
        assert!(msg.contains("extracting samples"));
    }

    #[test]
    fn test_vmaf_progress_sample_extract_total_zero() {
        // Arrange — total = 0 edge case
        let evt = dtvmgr_vmaf::SearchProgress::SampleExtract {
            current: 0,
            total: 0,
        };

        // Act
        let (pct, _msg) = vmaf_progress_to_stage(&evt, 0.0);

        // Assert — should not panic, pct = 0
        assert!((pct - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_vmaf_progress_scoring_monotonic_across_iterations() {
        // Arrange — iter 1 scoring vs iter 2 encoding
        let score_iter1 = dtvmgr_vmaf::SearchProgress::Scoring {
            iteration: 1,
            quality: 23.0,
            sample: 5,
            total: 5,
        };
        let enc_iter2 = dtvmgr_vmaf::SearchProgress::Encoding {
            iteration: 2,
            quality: 20.0,
            sample: 1,
            total: 5,
        };

        // Act
        let (pct1, _) = vmaf_progress_to_stage(&score_iter1, 0.0);
        let (pct2, _) = vmaf_progress_to_stage(&enc_iter2, 0.0);

        // Assert — iter 2 should have higher progress than iter 1
        assert!(pct2 > pct1, "iter 2 ({pct2}) should be > iter 1 ({pct1})");
    }

    #[test]
    fn test_vmaf_progress_iteration_result_late() {
        // Arrange — iteration beyond EST_ITERS (6)
        let evt = dtvmgr_vmaf::SearchProgress::IterationResult {
            iteration: 10,
            quality: 15.0,
            vmaf: 96.0,
            size_percent: 55.0,
        };

        // Act
        let (pct, msg) = vmaf_progress_to_stage(&evt, 0.0);

        // Assert — clamped to 1.0
        assert!(pct <= 1.0, "pct should be clamped to 1.0, got {pct}");
        assert!(msg.contains("iter 10"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_pipeline_inner_with_progress_callback() {
        // Arrange — test the on_progress callback path
        let (_tmp, input_ts, config) = setup_pipeline_fixture(false);

        let ctx = PipelineContext {
            input: input_ts,
            channel_name: Some("TEST".to_owned()),
            config,
            encode: false,
            target: AvsTarget::CutCm,
            add_chapter: false,
            out_dir: None,
            out_name: None,
            out_extension: None,
            remove: false,
            progress_mode: None,
            skip_duration_check: true,
            force: true,
        };

        let events = std::cell::RefCell::new(Vec::new());
        let on_progress = |evt: ProgressEvent| {
            events.borrow_mut().push(format!("{evt:?}"));
        };

        let mut summary = PipelineSummary::default();

        // Act
        let result = run_pipeline_inner(&ctx, Some(&on_progress), &mut summary);

        // Assert
        assert!(
            result.is_ok(),
            "pipeline with progress failed: {:#}",
            result.unwrap_err()
        );
        let captured = events.borrow();
        assert!(
            !captured.is_empty(),
            "expected progress events to be captured"
        );
        // Should end with Finished event
        assert!(
            captured.last().unwrap().contains("Finished"),
            "last event should be Finished, got: {}",
            captured.last().unwrap()
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_pipeline_inner_progress_force_false_skips() {
        // Arrange — first run populates cache, second run with progress + force=false
        let (_tmp, input_ts, config) = setup_pipeline_fixture(false);

        let ctx_first = PipelineContext {
            input: input_ts,
            channel_name: Some("TEST".to_owned()),
            config,
            encode: false,
            target: AvsTarget::CutCmLogo,
            add_chapter: false,
            out_dir: None,
            out_name: None,
            out_extension: None,
            remove: false,
            progress_mode: None,
            skip_duration_check: true,
            force: true,
        };

        let mut summary1 = PipelineSummary::default();
        let result1 = run_pipeline_inner(&ctx_first, None, &mut summary1);
        assert!(
            result1.is_ok(),
            "first run failed: {:#}",
            result1.unwrap_err()
        );

        // Second run with force=false and progress callback
        let ctx_second = PipelineContext {
            force: false,
            ..ctx_first
        };

        let events = std::cell::RefCell::new(Vec::new());
        let on_progress = |evt: ProgressEvent| {
            events.borrow_mut().push(format!("{evt:?}"));
        };

        let mut summary2 = PipelineSummary::default();

        // Act
        let result2 = run_pipeline_inner(&ctx_second, Some(&on_progress), &mut summary2);

        // Assert — should succeed with skipped steps (no progress events for skipped steps)
        assert!(
            result2.is_ok(),
            "second run failed: {:#}",
            result2.unwrap_err()
        );
        let captured = events.borrow();
        // Should still have a Finished event
        assert!(
            captured.last().unwrap().contains("Finished"),
            "last event should be Finished"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_pipeline_inner_with_filter_and_progress() {
        // Arrange — filter generation path with progress callback
        let (_tmp, input_ts, config) = setup_pipeline_fixture(false);

        let ctx = PipelineContext {
            input: input_ts,
            channel_name: Some("TEST".to_owned()),
            config,
            encode: false,
            target: AvsTarget::CutCmLogo,
            add_chapter: false,
            out_dir: None,
            out_name: None,
            out_extension: None,
            remove: false,
            progress_mode: None,
            skip_duration_check: true,
            force: true,
        };

        let events = std::cell::RefCell::new(Vec::new());
        let on_progress = |evt: ProgressEvent| {
            events.borrow_mut().push(format!("{evt:?}"));
        };

        let mut summary = PipelineSummary::default();

        // Act
        let result = run_pipeline_inner(&ctx, Some(&on_progress), &mut summary);

        // Assert
        assert!(
            result.is_ok(),
            "pipeline with filter + progress failed: {:#}",
            result.unwrap_err()
        );
    }

    // ── canonicalize_dirs (additional) ────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_canonicalize_dirs_with_symlinks() {
        // Arrange: create real dirs and symlink
        let tmp = tempfile::tempdir().unwrap();
        let real_jl = tmp.path().join("real_jl");
        let real_logo = tmp.path().join("real_logo");
        let real_result = tmp.path().join("real_result");
        std::fs::create_dir_all(&real_jl).unwrap();
        std::fs::create_dir_all(&real_logo).unwrap();
        std::fs::create_dir_all(&real_result).unwrap();

        let link_jl = tmp.path().join("link_jl");
        std::os::unix::fs::symlink(&real_jl, &link_jl).unwrap();

        let dirs = JlseDirs {
            jl: link_jl,
            logo: real_logo,
            result: real_result,
        };

        // Act
        let canon = canonicalize_dirs(&dirs).unwrap();

        // Assert: symlink resolved to real path
        assert_eq!(canon.jl, std::fs::canonicalize(&real_jl).unwrap());
        assert!(canon.jl.is_absolute());
        assert!(canon.logo.is_absolute());
        assert!(canon.result.is_absolute());
    }

    // ── validate_encode_config (additional) ───────────────────

    #[test]
    fn test_validate_encode_config_qs_enabled_qp_flag_conflict() {
        // Arrange: -qp flag conflicts with quality_search
        let enc = encode_with_qs(true, vec!["-qp".to_owned(), "30".to_owned()]);

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("-qp"), "expected '-qp' in error: {msg}");
    }

    #[test]
    fn test_validate_encode_config_qs_enabled_cq_colon_v_conflict() {
        // Arrange: -cq:v flag conflicts with quality_search
        let enc = encode_with_qs(true, vec!["-cq:v".to_owned(), "25".to_owned()]);

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("-cq:v"), "expected '-cq:v' in error: {msg}");
    }

    #[test]
    fn test_validate_encode_config_no_video_section_qs_enabled() {
        // Arrange: quality_search enabled but no video section
        let enc = JlseEncode {
            format: Some("mkv".to_owned()),
            input: None,
            video: None,
            audio: None,
            duration_check: None,
            quality_search: Some(crate::types::QualitySearchConfig {
                enabled: true,
                target_vmaf: None,
                max_encoded_percent: None,
                min_vmaf_tolerance: None,
                thorough: None,
                sample_duration_secs: None,
                skip_secs: None,
                sample_every_secs: None,
                min_samples: None,
                max_samples: None,
                vmaf_subsample: None,
            }),
        };

        // Act
        let result = validate_encode_config(Some(&enc));

        // Assert: no video section means no conflict
        assert!(result.is_ok());
    }

    // ── resolve_output_path (additional) ──────────────────────

    #[test]
    fn test_resolve_output_path_m2ts_extension_replaced() {
        // Arrange
        let input = Path::new("/rec/recording.m2ts");

        // Act
        let result = resolve_output_path(input, None, None, "mkv");

        // Assert
        assert_eq!(result, PathBuf::from("/rec/recording.mkv"));
    }

    #[test]
    fn test_resolve_output_path_no_parent() {
        // Arrange: input with no parent directory
        let input = Path::new("recording.ts");

        // Act
        let result = resolve_output_path(input, None, None, "mkv");

        // Assert
        assert_eq!(result, PathBuf::from("recording.mkv"));
    }

    // ── vmaf_progress_to_stage (additional) ───────────────────

    #[test]
    fn test_vmaf_progress_sample_extract_both_zero() {
        // Arrange: total=0 edge case, should not divide by zero
        let evt = dtvmgr_vmaf::SearchProgress::SampleExtract {
            current: 0,
            total: 0,
        };

        // Act
        let (pct, msg) = vmaf_progress_to_stage(&evt, 0.0);

        // Assert
        assert!((pct - 0.0).abs() < f64::EPSILON);
        assert!(msg.contains("(0/0)"));
    }

    #[test]
    fn test_vmaf_progress_encoding_total_zero_edge() {
        // Arrange: total=0 edge case
        let evt = dtvmgr_vmaf::SearchProgress::Encoding {
            iteration: 1,
            quality: 23.0,
            sample: 1,
            total: 0,
        };

        // Act
        let (pct, _msg) = vmaf_progress_to_stage(&evt, 0.0);

        // Assert: should not panic, progress clamped
        assert!(pct >= 0.0);
        assert!(pct <= 1.0);
    }

    // ── round1 / round3 (additional) ─────────────────────────

    #[test]
    fn test_round1_negative() {
        assert!((round1(-2.35) - (-2.4_f64)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_round3_large_value() {
        assert!((round3(100.0) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_round3_tiny_value() {
        assert!((round3(0.001) - 0.001).abs() < 1e-12);
    }

    // ── quality search cache helpers ─────────────────────────

    /// Build a minimal `SearchConfig` for cache-related unit tests.
    fn make_search_config() -> dtvmgr_vmaf::SearchConfig {
        dtvmgr_vmaf::SearchConfig {
            ffmpeg_bin: PathBuf::from("ffmpeg"),
            input_file: PathBuf::from("input.ts"),
            content_segments: vec![dtvmgr_vmaf::ContentSegment {
                start_secs: 0.0,
                end_secs: 1440.0,
            }],
            encoder: dtvmgr_vmaf::EncoderConfig::libx264(),
            video_filter: String::from("yadif,scale=1280:720"),
            target_vmaf: 93.0,
            max_encoded_percent: 80.0,
            min_vmaf_tolerance: 1.0,
            thorough: true,
            sample: dtvmgr_vmaf::SampleConfig::default(),
            extra_encode_args: Vec::new(),
            extra_input_args: Vec::new(),
            reference_filter: None,
            temp_dir: None,
        }
    }

    #[test]
    fn build_cache_config_reflects_search_config() {
        // Arrange
        let sc = make_search_config();
        let avs = "Trim(0,1000)++Trim(2000,5000)";

        // Act
        let cc = build_cache_config(&sc, avs);

        // Assert — spot-check key fields
        assert_eq!(cc.codec, "libx264");
        assert_eq!(cc.video_filter, "yadif,scale=1280:720");
        assert!((cc.target_vmaf - 93.0).abs() < f32::EPSILON);
        assert!((cc.max_encoded_percent - 80.0).abs() < f32::EPSILON);
        assert_eq!(cc.avs_content, avs);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn quality_cache_roundtrip() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("obs_quality_search.json");
        let sc = make_search_config();
        let cc = build_cache_config(&sc, "Trim(0,1000)");
        let result = dtvmgr_vmaf::SearchResult {
            quality_value: 25.0,
            quality_param: String::from("-crf"),
            mean_vmaf: 94.5,
            predicted_size_percent: 65.0,
            iterations: 7,
        };

        // Act — save then load
        save_quality_cache(&cache_path, &cc, &result);
        let loaded = load_quality_cache(&cache_path, &cc).unwrap();

        // Assert
        assert!((loaded.quality_value - 25.0).abs() < f32::EPSILON);
        assert_eq!(loaded.quality_param, "-crf");
        assert!((loaded.mean_vmaf - 94.5).abs() < f32::EPSILON);
        assert!((loaded.predicted_size_percent - 65.0).abs() < f64::EPSILON);
        assert_eq!(loaded.iterations, 7);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn quality_cache_invalidated_on_config_change() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("obs_quality_search.json");
        let sc = make_search_config();
        let cc = build_cache_config(&sc, "Trim(0,1000)");
        let result = dtvmgr_vmaf::SearchResult {
            quality_value: 25.0,
            quality_param: String::from("-crf"),
            mean_vmaf: 94.5,
            predicted_size_percent: 65.0,
            iterations: 7,
        };
        save_quality_cache(&cache_path, &cc, &result);

        // Different avs_content → different cache config
        let cc_changed = build_cache_config(&sc, "Trim(0,2000)");

        // Act
        let loaded = load_quality_cache(&cache_path, &cc_changed);

        // Assert — stale cache is discarded
        assert!(loaded.is_none());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn quality_cache_missing_file_returns_none() {
        // Arrange — path that does not exist
        let path = Path::new("/tmp/nonexistent_obs_quality_search.json");

        // Act / Assert
        let sc = make_search_config();
        let cc = build_cache_config(&sc, "Trim(0,1000)");
        assert!(load_quality_cache(path, &cc).is_none());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn quality_cache_malformed_json_returns_none() {
        // Arrange — write garbage JSON to cache file
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("obs_quality_search.json");
        std::fs::write(&cache_path, b"not valid json {{{").unwrap();

        let sc = make_search_config();
        let cc = build_cache_config(&sc, "Trim(0,1000)");

        // Act / Assert — parse failure returns None gracefully
        assert!(load_quality_cache(&cache_path, &cc).is_none());
    }
}
