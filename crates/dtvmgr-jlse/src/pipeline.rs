//! Pipeline orchestration for the CM detection workflow.
//!
//! Executes all pipeline steps in order: input validation, channel
//! detection, parameter detection, external command execution, AVS
//! concatenation, and chapter generation.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tracing::{debug, info};

use crate::avs;
use crate::channel;
use crate::command;
use crate::output;
use crate::param;
use crate::settings::{BinaryPaths, DataPaths, OutputPaths, init_output_paths};
use crate::types::{AvsTarget, Channel, DetectionParam, JlseConfig, JlseDirs};

// ── Types ────────────────────────────────────────────────────

/// Pipeline execution context.
#[allow(clippy::module_name_repetitions, clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct PipelineContext {
    /// Input TS file path.
    pub input: PathBuf,
    /// Channel name override.
    pub channel_name: Option<String>,
    /// Whether to run tsdivider preprocessing.
    pub tsdivider: bool,
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
    /// Remove intermediate files after processing.
    pub remove: bool,
    /// Metadata for encoded file (title, description, extended).
    pub metadata: command::ffmpeg::FfmpegMetadata,
}

// ── Pipeline ─────────────────────────────────────────────────

/// Run the full CM detection pipeline.
///
/// # Errors
///
/// Returns an error if any pipeline step fails (validation, file I/O,
/// external command execution, etc.).
#[allow(clippy::module_name_repetitions, clippy::too_many_lines)]
pub fn run_pipeline(ctx: &PipelineContext) -> Result<()> {
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

    let config_abs = JlseConfig {
        dirs,
        bins: ctx.config.bins.clone(),
    };
    let bins = BinaryPaths::from_config(&config_abs);
    let data = DataPaths::from_config(&config_abs);
    let paths = init_output_paths(&config_abs.dirs.result, &filename)?;

    // Step 2: Channel detection
    let channels = channel::load_channels(&data.channel_list)?;
    let filepath = input.to_string_lossy();
    let detected_channel =
        channel::detect_channel(&channels, &filepath, ctx.channel_name.as_deref());
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

    // Step 5: (optional) tsdivider
    let (actual_input, stream_index) = if ctx.tsdivider {
        info!("running tsdivider");
        command::tsdivider::run(&bins.tsdivider, &input, &paths.tsdivider_output)?;
        (paths.tsdivider_output.clone(), avs::STREAM_INDEX_TSDIVIDER)
    } else {
        (input.clone(), avs::STREAM_INDEX_NORMAL)
    };

    // Step 6: AVS generation
    avs::create(&paths.input_avs, &actual_input, stream_index)?;
    debug!(path = %paths.input_avs.display(), "created input AVS");

    // Step 7: chapter_exe
    command::chapter_exe::run(
        &bins.chapter_exe,
        &paths.input_avs,
        &paths.chapterexe_output,
    )?;
    debug!("chapter_exe completed");

    // Step 8: logoframe
    command::logoframe::run(
        &bins.logoframe,
        &paths.input_avs,
        &paths.logoframe_txt_output,
        &paths.logoframe_avs_output,
        &config_abs.dirs.logo,
        detected_channel.as_ref(),
    )?;
    debug!("logoframe completed");

    // Step 9: join_logo_scp
    let jl_command_file = config_abs.dirs.jl.join(&det_param.jl_run);
    command::join_logo_scp::run(
        &bins.join_logo_scp,
        &paths.logoframe_txt_output,
        &paths.chapterexe_output,
        &jl_command_file,
        &paths.output_avs_cut,
        &paths.jlscp_output,
        &det_param,
    )?;
    debug!("join_logo_scp completed");

    // Step 10: AVS concatenation
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

    // Step 11: Chapter generation
    generate_chapters(&paths)?;

    // Step 12: (optional) FFmpeg filter generation
    if ctx.filter {
        info!("generating ffmpeg filter");
        let fps = command::ffprobe::get_frame_rate(&bins.ffprobe, &actual_input)?;
        output::ffmpeg_filter::create(&paths.output_avs_cut, &paths.output_filter_cut, &fps)?;
        debug!("ffmpeg filter generation completed");
    }

    // Step 13: (optional) FFmpeg encoding
    if ctx.encode {
        info!("running ffmpeg encoding");
        let avs_file = select_avs_file(&paths, ctx.target);
        let output_mp4 =
            resolve_output_path(&input, ctx.out_dir.as_deref(), ctx.out_name.as_deref());
        let chapter_file = if ctx.add_chapter {
            Some(paths.file_txt_cpt_cut.as_path())
        } else {
            None
        };
        command::ffmpeg::run(
            &bins.ffmpeg,
            avs_file,
            &output_mp4,
            chapter_file,
            &ctx.metadata,
            ctx.ffmpeg_option.as_deref().unwrap_or(""),
        )?;
        debug!("ffmpeg encoding completed");
    }

    // Step 14: (optional) Remove intermediate files
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

    info!("pipeline completed successfully");
    Ok(())
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

/// Resolve the output MP4 file path.
///
/// - `out_dir` overrides the parent directory of the input file.
/// - `out_name` overrides the file stem.
/// - Extension is always `.mp4`.
fn resolve_output_path(input: &Path, out_dir: Option<&Path>, out_name: Option<&str>) -> PathBuf {
    let dir = out_dir.unwrap_or_else(|| input.parent().unwrap_or_else(|| Path::new(".")));
    let stem = out_name.map_or_else(
        || input.file_stem().unwrap_or_default().to_string_lossy(),
        std::borrow::Cow::Borrowed,
    );
    dir.join(format!("{stem}.mp4"))
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
        let result = resolve_output_path(input, None, None);

        // Assert
        assert_eq!(result, PathBuf::from("/rec/recording.mp4"));
    }

    #[test]
    fn test_resolve_output_path_with_outdir() {
        // Arrange
        let input = Path::new("/rec/recording.ts");

        // Act
        let result = resolve_output_path(input, Some(Path::new("/enc")), None);

        // Assert
        assert_eq!(result, PathBuf::from("/enc/recording.mp4"));
    }

    #[test]
    fn test_resolve_output_path_with_outname() {
        // Arrange
        let input = Path::new("/rec/recording.ts");

        // Act
        let result = resolve_output_path(input, None, Some("custom"));

        // Assert
        assert_eq!(result, PathBuf::from("/rec/custom.mp4"));
    }

    #[test]
    fn test_resolve_output_path_with_outdir_and_outname() {
        // Arrange
        let input = Path::new("/rec/recording.ts");

        // Act
        let result = resolve_output_path(input, Some(Path::new("/enc")), Some("custom"));

        // Assert
        assert_eq!(result, PathBuf::from("/enc/custom.mp4"));
    }
}
