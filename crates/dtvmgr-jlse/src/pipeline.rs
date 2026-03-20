//! Pipeline orchestration for the CM detection workflow.
//!
//! Executes all pipeline steps in order: input validation, channel
//! detection, parameter detection, external command execution, AVS
//! concatenation, chapter generation, and optional EIT-based MKV encoding.

use std::cell::Cell;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tracing::{debug, info, instrument};

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
    /// Generate ffmpeg `filter_complex` file.
    pub filter: bool,
    /// Run ffmpeg encoding.
    pub encode: bool,
    /// Encode target AVS selection.
    pub target: AvsTarget,
    /// Add chapter metadata to encoded file.
    pub add_chapter: bool,
    /// Additional ffmpeg options.
    pub ffmpeg_option: Option<String>,
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
}

// ── Pipeline ─────────────────────────────────────────────────

/// Run the full CM detection pipeline.
///
/// # Errors
///
/// Returns an error if any pipeline step fails (validation, file I/O,
/// external command execution, etc.).
#[instrument(skip_all, err(level = "error"))]
#[allow(
    clippy::module_name_repetitions,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub fn run_pipeline(
    ctx: &PipelineContext,
    on_progress: Option<&dyn Fn(ProgressEvent)>,
) -> Result<()> {
    // Step 1: Validate input extension
    validate_input(&ctx.input)?;

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

    // Step 6: chapter_exe (stages 1-2: lwi index + mute detection)
    if let Some(cb) = on_progress {
        cb(ProgressEvent::StageStart {
            stage: 1,
            total: 4,
            name: "lwi index".to_owned(),
        });
        let in_mute_phase = Cell::new(false);
        let total_frames = Cell::new(0u32);
        let enter_mute_phase = || {
            if !in_mute_phase.get() {
                in_mute_phase.set(true);
                cb(ProgressEvent::StageStart {
                    stage: 2,
                    total: 4,
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
    if let Some(cb) = on_progress {
        cb(ProgressEvent::StageStart {
            stage: 3,
            total: 4,
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
    if let Some(cb) = on_progress {
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

    // Step 11: (optional) FFmpeg filter generation
    if ctx.filter {
        info!("generating ffmpeg filter");
        let fps = command::ffprobe::frame_rate(&bins.ffprobe, &input)?;
        output::ffmpeg_filter::create(&paths.output_avs_cut, &paths.output_filter_cut, &fps)?;
        debug!("ffmpeg filter generation completed");
    }

    // Resolve FFmpeg encode args (always logged for visibility)
    let (input_encode_args, output_encode_args) =
        JlseEncode::build_encode_args(ctx.config.encode.as_ref(), ctx.ffmpeg_option.as_deref());
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
        let mkv_metadata = extract_eit_for_mkv(&bins.tstables, &input, &paths.save_dir)
            .context("EIT extraction is required for MKV encoding")?;

        let avs_file = select_avs_file(&paths, ctx.target);

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
                stage: 4,
                total: 4,
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

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

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
}
