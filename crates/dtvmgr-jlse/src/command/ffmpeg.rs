//! Wrapper for the `ffmpeg` external command.
//!
//! Encodes media files into MKV with optional chapter metadata and
//! EIT XML attachment.

use std::ffi::OsStr;
use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use dtvmgr_tsduck::command::apply_pdeathsig;
use tracing::{info, instrument};

use crate::progress::{self, ProgressEvent};
use crate::types::JlseEncode;

/// Options that should NOT receive automatic stream specifiers
/// because they are muxer-level or global options.
const GLOBAL_OPTIONS: &[&str] = &[
    "-movflags",
    "-max_muxing_queue_size",
    "-f",
    "-t",
    "-ss",
    "-to",
    "-threads",
    "-shortest",
    "-fflags",
    "-flags",
];

/// Append extra args, automatically adding a stream specifier (`:v` or `:a`)
/// to option flags that lack one — unless the flag is a known global/muxer option.
fn append_extra_with_specifier(args: &mut Vec<String>, extra: &[String], specifier: &str) {
    for arg in extra {
        if arg.starts_with('-') && !arg.contains(':') && !GLOBAL_OPTIONS.contains(&arg.as_str()) {
            args.push(format!("{arg}:{specifier}"));
        } else {
            args.push(arg.clone());
        }
    }
}

// ── Types ────────────────────────────────────────────────────

/// Metadata fields for the encoded MKV file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MkvMetadata {
    /// `TITLE` tag (`program_name` from `short_event_descriptor`).
    pub title: Option<String>,
    /// `SUBTITLE` tag (`text` from `short_event_descriptor`).
    pub subtitle: Option<String>,
    /// `DESCRIPTION` tag (concatenated extended event text).
    pub description: Option<String>,
    /// `GENRE` tag (ARIB genre name in English).
    pub genre: Option<String>,
    /// `DATE_RECORDED` tag (event `start_time`).
    pub date_recorded: Option<String>,
    /// Path to EIT XML file for attachment.
    pub eit_xml_path: Option<PathBuf>,
}

// ── Functions ────────────────────────────────────────────────

/// Run ffmpeg encoding with the given parameters.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits
/// with a non-zero status code.
#[instrument(skip_all, err(level = "error"))]
pub fn run(
    binary: &Path,
    avs_file: &Path,
    output_file: &Path,
    chapter_file: Option<&Path>,
    metadata: &MkvMetadata,
    input_options: &str,
    extra_options: &str,
) -> Result<()> {
    let args = build_args(
        avs_file,
        output_file,
        chapter_file,
        metadata,
        input_options,
        extra_options,
    );
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    super::run(binary, &os_args)
}

/// Build the argument list for ffmpeg (MKV output).
///
/// Format:
/// ```text
/// ffmpeg [input_options...] -y -i <avs_file>
///        [-i <chapter> -map_metadata 1]
///        [-metadata TITLE=... -metadata SUBTITLE=... -metadata GENRE=...
///         -metadata DESCRIPTION=... -metadata DATE_RECORDED=...]
///        [-attach eit.xml -metadata:s:t:0 mimetype=application/xml
///         -metadata:s:t:0 filename=eit.xml]
///        [extra_options...]
///        <output.mkv>
/// ```
#[must_use]
pub fn build_args(
    avs_file: &Path,
    output_file: &Path,
    chapter_file: Option<&Path>,
    metadata: &MkvMetadata,
    input_options: &str,
    extra_options: &str,
) -> Vec<String> {
    let mut args = Vec::new();

    // Input options (hwaccel, fflags, etc.) must come before -i
    for opt in input_options.split_whitespace() {
        args.push(opt.to_owned());
    }

    args.push("-y".to_owned());
    args.push("-i".to_owned());
    args.push(avs_file.display().to_string());

    if let Some(chapter) = chapter_file {
        args.push("-i".to_owned());
        args.push(chapter.display().to_string());
        args.push("-map_metadata".to_owned());
        args.push("1".to_owned());
    }

    // Standard MKV metadata tags
    push_metadata_tag(&mut args, "TITLE", metadata.title.as_deref());
    push_metadata_tag(&mut args, "SUBTITLE", metadata.subtitle.as_deref());
    push_metadata_tag(&mut args, "DESCRIPTION", metadata.description.as_deref());
    push_metadata_tag(&mut args, "GENRE", metadata.genre.as_deref());
    push_metadata_tag(
        &mut args,
        "DATE_RECORDED",
        metadata.date_recorded.as_deref(),
    );

    // EIT XML attachment
    if let Some(ref xml_path) = metadata.eit_xml_path {
        args.push("-attach".to_owned());
        args.push(xml_path.display().to_string());
        args.push("-metadata:s:t:0".to_owned());
        args.push("mimetype=application/xml".to_owned());
        args.push("-metadata:s:t:0".to_owned());
        args.push("filename=eit.xml".to_owned());
    }

    for opt in extra_options.split_whitespace() {
        args.push(opt.to_owned());
    }

    args.push(output_file.display().to_string());

    args
}

/// Push a `-metadata TAG=value` pair if value is non-empty.
fn push_metadata_tag(args: &mut Vec<String>, tag: &str, value: Option<&str>) {
    if let Some(v) = value
        && !v.is_empty()
    {
        args.push("-metadata".to_owned());
        args.push(format!("{tag}={v}"));
    }
}

// ── JlseEncode → FFmpeg args ─────────────────────────────────

impl JlseEncode {
    /// Build args placed **before** `-i` (input-side options).
    ///
    /// Generated order:
    /// 1. Global flags (`-hide_banner`, `-ignore_unknown`)
    /// 2. HW device init (`-init_hw_device`, `-filter_hw_device`)
    /// 3. Input flags (`-fflags`, `-analyzeduration`, `-probesize`)
    /// 4. HW accel (`-hwaccel_output_format`, `-hwaccel`, `-c:v` decoder)
    #[must_use]
    pub fn to_input_args(&self) -> Vec<String> {
        let mut args = vec!["-hide_banner".to_owned(), "-ignore_unknown".to_owned()];

        if let Some(ref input) = self.input {
            // HW device init (must precede input flags)
            if let Some(ref hw) = input.init_hw_device {
                args.push("-init_hw_device".to_owned());
                args.push(hw.clone());
            }
            if let Some(ref hw) = input.filter_hw_device {
                args.push("-filter_hw_device".to_owned());
                args.push(hw.clone());
            }

            // Input flags
            if let Some(ref flags) = input.flags {
                args.push("-fflags".to_owned());
                args.push(flags.clone());
            }
            if let Some(ref dur) = input.analyzeduration {
                args.push("-analyzeduration".to_owned());
                args.push(dur.clone());
            }
            if let Some(ref size) = input.probesize {
                args.push("-probesize".to_owned());
                args.push(size.clone());
            }

            // HW accel
            if let Some(ref fmt) = input.hwaccel_output_format {
                args.push("-hwaccel_output_format".to_owned());
                args.push(fmt.clone());
            }
            if let Some(ref accel) = input.hwaccel {
                args.push("-hwaccel".to_owned());
                args.push(accel.clone());
            }
            if let Some(ref dec) = input.decoder {
                args.push("-c:v".to_owned());
                args.push(dec.clone());
            }
        }

        args
    }

    /// Build args placed **after** `-i` (output-side options).
    ///
    /// Generated order:
    /// 1. Video mapping / codec / preset / profile / `pix_fmt` / filter / extra
    /// 2. Audio mapping / codec / `sample_rate` / bitrate / channels / extra
    #[must_use]
    #[allow(clippy::cognitive_complexity)]
    pub fn to_output_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        // Video
        if let Some(ref video) = self.video {
            let has_settings = video.codec.is_some()
                || video.preset.is_some()
                || video.profile.is_some()
                || video.pix_fmt.is_some()
                || video.aspect.is_some()
                || video.filter.is_some()
                || !video.extra.is_empty();
            if has_settings {
                args.push("-map".to_owned());
                args.push("0:v".to_owned());
            }
            if let Some(ref aspect) = video.aspect {
                args.push("-aspect".to_owned());
                args.push(aspect.clone());
            }
            if let Some(ref codec) = video.codec {
                args.push("-c:v".to_owned());
                args.push(codec.clone());
            }
            if let Some(ref preset) = video.preset {
                args.push("-preset".to_owned());
                args.push(preset.clone());
            }
            if let Some(ref profile) = video.profile {
                args.push("-profile:v".to_owned());
                args.push(profile.clone());
            }
            if let Some(ref pix_fmt) = video.pix_fmt {
                args.push("-pix_fmt".to_owned());
                args.push(pix_fmt.clone());
            }
            if let Some(ref filter) = video.filter {
                args.push("-vf".to_owned());
                let needs_hwupload = self
                    .input
                    .as_ref()
                    .and_then(|i| i.filter_hw_device.as_ref())
                    .is_some();
                if needs_hwupload {
                    args.push(prepare_hw_filter(filter, video.pix_fmt.as_deref()));
                } else {
                    args.push(filter.clone());
                }
            }
            append_extra_with_specifier(&mut args, &video.extra, "v");
        }

        // Audio
        if let Some(ref audio) = self.audio {
            let has_settings = audio.codec.is_some()
                || audio.sample_rate.is_some()
                || audio.bitrate.is_some()
                || audio.channels.is_some()
                || !audio.extra.is_empty();
            if has_settings {
                args.push("-map".to_owned());
                args.push("0:a".to_owned());
            }
            if let Some(ref codec) = audio.codec {
                args.push("-c:a".to_owned());
                args.push(codec.clone());
            }
            if let Some(rate) = audio.sample_rate {
                args.push("-ar".to_owned());
                args.push(rate.to_string());
            }
            if let Some(ref bitrate) = audio.bitrate {
                args.push("-b:a".to_owned());
                args.push(bitrate.clone());
            }
            if let Some(channels) = audio.channels {
                args.push("-ac".to_owned());
                args.push(channels.to_string());
            }
            append_extra_with_specifier(&mut args, &audio.extra, "a");
        }

        args
    }

    /// Build encode args from an optional TOML config.
    ///
    /// Returns `(input_args, output_args)` — input args go before `-i`,
    /// output args go after `-i`.
    #[must_use]
    pub fn build_encode_args(encode: Option<&Self>) -> (Vec<String>, Vec<String>) {
        encode.map_or_else(
            || (Vec::new(), Vec::new()),
            |e| (e.to_input_args(), e.to_output_args()),
        )
    }
}

// ── VPP filter helpers ──────────────────────────────────────

/// Check whether a VPP filter segment contains a `format=` parameter.
///
/// VPP parameters use `:` as separator (e.g.
/// `vpp_qsv=deinterlace=advanced:format=p010le:height=720`).
/// Returns `true` when any `vpp_qsv` segment contains `:format=`.
fn vpp_segment_has_format(filter: &str) -> bool {
    filter.split(',').any(|seg| {
        let seg = seg.trim();
        seg.starts_with("vpp_qsv") && seg.contains(":format=")
    })
}

/// Validate that no VPP segment contains an explicit `format=`
/// parameter.
///
/// When `filter_hw_device` is set, `pix_fmt` is the single source
/// of truth for pixel format.  Specifying `format=` inside VPP
/// creates a redundant (and potentially inconsistent) setting.
///
/// # Errors
///
/// Returns an error with a user-friendly message when `format=`
/// is found inside a VPP filter segment.
pub(crate) fn validate_vpp_no_format(filter: &str) -> Result<()> {
    if vpp_segment_has_format(filter) {
        bail!(
            "VPP filter contains `:format=…` which is redundant with `pix_fmt`. \
             Remove `:format=…` from the VPP parameters — the pixel format \
             is derived automatically from `[jlse.encode.video] pix_fmt`"
        );
    }
    Ok(())
}

/// Prepare a video filter string for HW-accelerated encoding.
///
/// 1. Prepends `hwupload=extra_hw_frames=64,` when
///    `filter_hw_device` is set and the filter does not already
///    start with `hwupload`.
/// 2. Injects `format={pix_fmt}` into each `vpp_qsv` segment so
///    that the VPP output surface matches the encoder pixel format.
pub(crate) fn prepare_hw_filter(filter: &str, pix_fmt: Option<&str>) -> String {
    let injected = inject_vpp_format(filter, pix_fmt);

    let has_hwupload = injected
        .split(',')
        .any(|seg| seg.trim_start().starts_with("hwupload"));

    if has_hwupload {
        injected
    } else {
        format!("format=nv12,hwupload=extra_hw_frames=64,{injected}")
    }
}

/// Inject `format={pix_fmt}` into each `vpp_qsv` segment that
/// does not already contain it.
///
/// Example: `vpp_qsv=deinterlace=advanced:h=720:w=1280` with
/// `pix_fmt = "p010le"` becomes
/// `vpp_qsv=deinterlace=advanced:h=720:w=1280:format=p010le`.
fn inject_vpp_format(filter: &str, pix_fmt: Option<&str>) -> String {
    let Some(fmt) = pix_fmt else {
        return filter.to_owned();
    };

    filter
        .split(',')
        .map(|seg| {
            let trimmed = seg.trim();
            if trimmed.starts_with("vpp_qsv") && !trimmed.contains(":format=") {
                format!("{seg}:format={fmt}")
            } else {
                seg.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

// ── Progress-aware FFmpeg execution ─────────────────────────

/// Run `FFmpeg` encoding with stderr progress parsing.
///
/// Captures stderr line-by-line, parses `FFmpeg` progress output,
/// and emits EPGStation-compatible JSON to stdout when in progress mode.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits
/// with a non-zero status code.
#[instrument(skip_all, err(level = "error"))]
#[allow(clippy::too_many_arguments)]
pub fn run_with_progress(
    binary: &Path,
    avs_file: &Path,
    output_file: &Path,
    chapter_file: Option<&Path>,
    metadata: &MkvMetadata,
    input_options: &str,
    extra_options: &str,
    duration: f64,
    on_progress: &dyn Fn(ProgressEvent),
) -> Result<()> {
    let args = build_args(
        avs_file,
        output_file,
        chapter_file,
        metadata,
        input_options,
        extra_options,
    );
    info!(cmd = %binary.display(), ?args, "running ffmpeg with progress");

    let mut cmd = Command::new(binary);
    cmd.args(&args).stdout(Stdio::null()).stderr(Stdio::piped());
    apply_pdeathsig(&mut cmd);
    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn {}", binary.display()))?;

    // Read stderr for progress parsing.
    // FFmpeg uses bare `\r` (without `\n`) to overwrite progress lines,
    // so we treat both `\r` and `\n` as line terminators instead of
    // using `BufReader::lines()` which only splits on `\n`.
    if let Some(stderr) = child.stderr.take() {
        let mut reader = BufReader::new(stderr);
        let mut line_bytes: Vec<u8> = Vec::new();

        loop {
            let available = reader.fill_buf().context("failed to read ffmpeg stderr")?;
            if available.is_empty() {
                break;
            }

            // Scan the buffer for `\r` or `\n` line terminators,
            // copying non-terminator slices in bulk.
            // Bounds: `pos` <= `len`, `pos + delim` <= `len` because
            // `delim` comes from `position()` on `available[pos..]`.
            let buf_len = available.len();
            #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
            {
                let mut pos = 0;
                while pos < buf_len {
                    if let Some(delim) = available[pos..]
                        .iter()
                        .position(|&b| b == b'\r' || b == b'\n')
                    {
                        line_bytes.extend_from_slice(&available[pos..pos + delim]);
                        if !line_bytes.is_empty() {
                            let segment = String::from_utf8_lossy(&line_bytes);
                            let segment = segment.trim();
                            if !segment.is_empty() {
                                emit_ffmpeg_line(segment, duration, on_progress);
                            }
                            line_bytes.clear();
                        }
                        pos += delim + 1;
                    } else {
                        line_bytes.extend_from_slice(&available[pos..]);
                        break;
                    }
                }
            }
            reader.consume(buf_len);
        }

        // Flush any remaining bytes.
        if !line_bytes.is_empty() {
            let segment = String::from_utf8_lossy(&line_bytes);
            let segment = segment.trim();
            if !segment.is_empty() {
                emit_ffmpeg_line(segment, duration, on_progress);
            }
        }
    }

    let status = child
        .wait()
        .with_context(|| format!("failed to wait for {}", binary.display()))?;

    if !status.success() {
        bail!(
            "{} exited with {}",
            binary.display(),
            status
                .code()
                .map_or_else(|| "signal".to_owned(), |c| c.to_string()),
        );
    }

    Ok(())
}

/// Process a single `FFmpeg` stderr line and emit progress events.
fn emit_ffmpeg_line(segment: &str, duration: f64, on_progress: &dyn Fn(ProgressEvent)) {
    on_progress(ProgressEvent::Log(segment.to_owned()));
    if let Some(p) = progress::parse_ffmpeg_progress(segment, duration) {
        on_progress(ProgressEvent::Encoding {
            percent: p.percent,
            log: p.log,
        });
    }
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::cognitive_complexity
    )]

    use std::path::{Path, PathBuf};

    use crate::types::{EncodeAudio, EncodeInput, EncodeVideo};

    use super::*;

    // ── build_args ──────────────────────────────────────────

    #[test]
    fn test_build_args_no_chapter_no_metadata() {
        // Arrange
        let metadata = MkvMetadata::default();

        // Act
        let args = build_args(
            Path::new("/out/in_cutcm.avs"),
            Path::new("/enc/output.mkv"),
            None,
            &metadata,
            "",
            "",
        );

        // Assert
        assert_eq!(args[0], "-y");
        assert_eq!(args[1], "-i");
        assert_eq!(args[2], "/out/in_cutcm.avs");
        assert_eq!(args[3], "/enc/output.mkv");
        assert_eq!(args.len(), 4);
    }

    #[test]
    fn test_build_args_with_chapter_and_metadata() {
        // Arrange
        let metadata = MkvMetadata {
            title: Some("Test Show".to_owned()),
            subtitle: Some("Episode 1".to_owned()),
            description: Some("A test description".to_owned()),
            genre: Some("Animation/Special Effects".to_owned()),
            date_recorded: Some("2024-12-31 15:00:00".to_owned()),
            eit_xml_path: None,
        };

        // Act
        let args = build_args(
            Path::new("/out/in_cutcm.avs"),
            Path::new("/enc/output.mkv"),
            Some(Path::new("/out/chapter.txt")),
            &metadata,
            "",
            "",
        );

        // Assert
        assert_eq!(args[1], "-i");
        assert_eq!(args[2], "/out/in_cutcm.avs");
        assert_eq!(args[3], "-i");
        assert_eq!(args[4], "/out/chapter.txt");
        assert_eq!(args[5], "-map_metadata");
        assert_eq!(args[6], "1");
        assert_eq!(args[7], "-metadata");
        assert_eq!(args[8], "TITLE=Test Show");
        assert_eq!(args[9], "-metadata");
        assert_eq!(args[10], "SUBTITLE=Episode 1");
        assert_eq!(args[11], "-metadata");
        assert_eq!(args[12], "DESCRIPTION=A test description");
        assert_eq!(args[13], "-metadata");
        assert_eq!(args[14], "GENRE=Animation/Special Effects");
        assert_eq!(args[15], "-metadata");
        assert_eq!(args[16], "DATE_RECORDED=2024-12-31 15:00:00");
        assert_eq!(*args.last().unwrap(), "/enc/output.mkv");
    }

    #[test]
    fn test_build_args_with_eit_xml_attachment() {
        // Arrange
        let metadata = MkvMetadata {
            title: Some("Show".to_owned()),
            eit_xml_path: Some(PathBuf::from("/out/eit.xml")),
            ..MkvMetadata::default()
        };

        // Act
        let args = build_args(
            Path::new("/out/in_cutcm.avs"),
            Path::new("/enc/output.mkv"),
            None,
            &metadata,
            "",
            "",
        );

        // Assert
        assert!(args.contains(&"-attach".to_owned()));
        assert!(args.contains(&"/out/eit.xml".to_owned()));
        assert!(args.contains(&"-metadata:s:t:0".to_owned()));
        assert!(args.contains(&"mimetype=application/xml".to_owned()));
        assert!(args.contains(&"filename=eit.xml".to_owned()));
    }

    #[test]
    fn test_build_args_with_extra_options() {
        // Arrange
        let metadata = MkvMetadata::default();

        // Act
        let args = build_args(
            Path::new("/out/in_cutcm.avs"),
            Path::new("/enc/output.mkv"),
            None,
            &metadata,
            "",
            "-c:v libx264 -crf 23",
        );

        // Assert — extra options before output file
        let output_idx = args.len() - 1;
        assert_eq!(args[output_idx], "/enc/output.mkv");
        assert!(args.contains(&"-c:v".to_owned()));
        assert!(args.contains(&"libx264".to_owned()));
        assert!(args.contains(&"-crf".to_owned()));
        assert!(args.contains(&"23".to_owned()));
    }

    #[test]
    fn test_build_args_with_input_options() {
        // Arrange
        let metadata = MkvMetadata::default();

        // Act
        let args = build_args(
            Path::new("/out/in.avs"),
            Path::new("/out.mkv"),
            None,
            &metadata,
            "-hwaccel qsv -c:v mpeg2_qsv",
            "-c:v hevc_qsv",
        );

        // Assert — input options come before -y -i
        assert_eq!(args[0], "-hwaccel");
        assert_eq!(args[1], "qsv");
        assert_eq!(args[2], "-c:v");
        assert_eq!(args[3], "mpeg2_qsv");
        assert_eq!(args[4], "-y");
        assert_eq!(args[5], "-i");
        assert_eq!(args[6], "/out/in.avs");
        // output options and output file come after
        assert!(args.contains(&"hevc_qsv".to_owned()));
        assert_eq!(*args.last().unwrap(), "/out.mkv");
    }

    #[test]
    fn test_build_args_chapter_no_title() {
        // Arrange
        let metadata = MkvMetadata {
            description: Some("Desc".to_owned()),
            ..MkvMetadata::default()
        };

        // Act
        let args = build_args(
            Path::new("/out/in.avs"),
            Path::new("/out.mkv"),
            Some(Path::new("/chap.txt")),
            &metadata,
            "",
            "",
        );

        // Assert — no TITLE metadata, but DESCRIPTION present
        assert!(!args.iter().any(|a| a.starts_with("TITLE=")));
        assert!(args.iter().any(|a| a == "DESCRIPTION=Desc"));
    }

    #[test]
    fn test_build_args_empty_string_metadata_skipped() {
        // Arrange
        let metadata = MkvMetadata {
            title: Some(String::new()),
            subtitle: Some("Sub".to_owned()),
            ..MkvMetadata::default()
        };

        // Act
        let args = build_args(
            Path::new("/out/in.avs"),
            Path::new("/out.mkv"),
            None,
            &metadata,
            "",
            "",
        );

        // Assert — empty title is skipped, subtitle is present
        assert!(!args.iter().any(|a| a.starts_with("TITLE=")));
        assert!(args.iter().any(|a| a == "SUBTITLE=Sub"));
    }

    #[test]
    fn test_build_args_no_movflags() {
        // Arrange — MKV does not use movflags
        let metadata = MkvMetadata {
            title: Some("Show".to_owned()),
            ..MkvMetadata::default()
        };

        // Act
        let args = build_args(
            Path::new("/out/in.avs"),
            Path::new("/out.mkv"),
            Some(Path::new("/chap.txt")),
            &metadata,
            "",
            "",
        );

        // Assert
        assert!(!args.iter().any(|a| a.contains("movflags")));
    }

    // ── to_input_args / to_output_args ─────────────────────

    #[test]
    fn test_to_input_args_no_input() {
        // Arrange
        let encode = JlseEncode {
            input: None,
            video: None,
            audio: None,
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_input_args();

        // Assert — only global flags
        assert_eq!(args, vec!["-hide_banner", "-ignore_unknown"]);
    }

    #[test]
    fn test_to_output_args_no_sections() {
        // Arrange
        let encode = JlseEncode {
            video: None,
            audio: None,
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — no output args
        assert!(args.is_empty());
    }

    #[test]
    fn test_to_input_args_with_hwaccel() {
        // Arrange
        let encode = JlseEncode {
            input: Some(EncodeInput {
                flags: Some("+discardcorrupt+genpts".to_owned()),
                analyzeduration: Some("30M".to_owned()),
                probesize: Some("100M".to_owned()),
                init_hw_device: None,
                filter_hw_device: None,
                hwaccel: Some("qsv".to_owned()),
                hwaccel_output_format: Some("qsv".to_owned()),
                decoder: Some("mpeg2_qsv".to_owned()),
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_input_args();

        // Assert — global flags, input flags, hwaccel in correct order
        assert_eq!(args[0], "-hide_banner");
        assert_eq!(args[1], "-ignore_unknown");
        assert_eq!(args[2], "-fflags");
        assert_eq!(args[3], "+discardcorrupt+genpts");
        assert_eq!(args[4], "-analyzeduration");
        assert_eq!(args[5], "30M");
        assert_eq!(args[6], "-probesize");
        assert_eq!(args[7], "100M");
        assert_eq!(args[8], "-hwaccel_output_format");
        assert_eq!(args[9], "qsv");
        assert_eq!(args[10], "-hwaccel");
        assert_eq!(args[11], "qsv");
        assert_eq!(args[12], "-c:v");
        assert_eq!(args[13], "mpeg2_qsv");
    }

    #[test]
    fn test_to_input_args_without_hwaccel() {
        // Arrange
        let encode = JlseEncode {
            input: Some(EncodeInput {
                flags: Some("+discardcorrupt+genpts".to_owned()),
                analyzeduration: Some("30M".to_owned()),
                probesize: Some("100M".to_owned()),
                ..EncodeInput::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_input_args();

        // Assert — no hwaccel-related args
        assert!(!args.contains(&"-hwaccel".to_owned()));
        assert!(!args.contains(&"-hwaccel_output_format".to_owned()));
        assert!(!args.contains(&"-c:v".to_owned()));
    }

    #[test]
    fn test_to_input_args_with_hw_device_init() {
        // Arrange — AVS (SW decode) + av1_qsv (HW encode) scenario
        let encode = JlseEncode {
            input: Some(EncodeInput {
                flags: Some("+discardcorrupt+genpts".to_owned()),
                analyzeduration: Some("30M".to_owned()),
                probesize: Some("100M".to_owned()),
                init_hw_device: Some("qsv=hw".to_owned()),
                filter_hw_device: Some("hw".to_owned()),
                hwaccel: None,
                hwaccel_output_format: None,
                decoder: None,
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_input_args();

        // Assert — hw device init appears after global flags, before input flags
        assert_eq!(
            args,
            vec![
                "-hide_banner",
                "-ignore_unknown",
                "-init_hw_device",
                "qsv=hw",
                "-filter_hw_device",
                "hw",
                "-fflags",
                "+discardcorrupt+genpts",
                "-analyzeduration",
                "30M",
                "-probesize",
                "100M",
            ]
        );
    }

    #[test]
    fn test_to_output_args_full() {
        // Arrange
        let encode = JlseEncode {
            format: Some("mkv".to_owned()),
            input: Some(EncodeInput {
                flags: Some("+discardcorrupt+genpts".to_owned()),
                analyzeduration: Some("30M".to_owned()),
                probesize: Some("100M".to_owned()),
                ..EncodeInput::default()
            }),
            video: Some(EncodeVideo {
                codec: Some("libx264".to_owned()),
                preset: Some("medium".to_owned()),
                profile: Some("main".to_owned()),
                pix_fmt: Some("yuv420p".to_owned()),
                aspect: Some("16:9".to_owned()),
                filter: Some(
                    "yadif=mode=send_frame:parity=auto:deint=all,scale=w=1280:h=720".to_owned(),
                ),
                extra: vec![
                    "-crf".to_owned(),
                    "23".to_owned(),
                    "-movflags".to_owned(),
                    "faststart".to_owned(),
                ],
            }),
            audio: Some(EncodeAudio {
                codec: Some("aac".to_owned()),
                sample_rate: Some(48000),
                bitrate: Some("256k".to_owned()),
                channels: Some(2),
                extra: vec![],
            }),
            duration_check: None,
            quality_search: None,
        };

        // Act
        let args = encode.to_output_args();

        // Assert — no global/input flags in output args
        assert!(!args.contains(&"-hide_banner".to_owned()));
        assert!(!args.contains(&"-fflags".to_owned()));

        // Assert — video mapping and codec
        assert!(args.contains(&"-map".to_owned()));
        assert!(args.contains(&"0:v".to_owned()));
        assert!(args.contains(&"-c:v".to_owned()));
        assert!(args.contains(&"libx264".to_owned()));
        assert!(args.contains(&"-preset".to_owned()));
        assert!(args.contains(&"medium".to_owned()));
        assert!(args.contains(&"-profile:v".to_owned()));
        assert!(args.contains(&"main".to_owned()));
        assert!(args.contains(&"-pix_fmt".to_owned()));
        assert!(args.contains(&"yuv420p".to_owned()));
        assert!(args.contains(&"-aspect".to_owned()));
        assert!(args.contains(&"16:9".to_owned()));
        assert!(args.contains(&"-vf".to_owned()));
        assert!(args.contains(&"-crf:v".to_owned()));
        assert!(args.contains(&"23".to_owned()));
        // -movflags is a global option — must NOT get :v specifier
        assert!(args.contains(&"-movflags".to_owned()));

        // Assert — audio mapping and codec
        assert!(args.contains(&"0:a".to_owned()));
        assert!(args.contains(&"-c:a".to_owned()));
        assert!(args.contains(&"aac".to_owned()));
        assert!(args.contains(&"-ar".to_owned()));
        assert!(args.contains(&"48000".to_owned()));
        assert!(args.contains(&"-b:a".to_owned()));
        assert!(args.contains(&"256k".to_owned()));
        assert!(args.contains(&"-ac".to_owned()));
        assert!(args.contains(&"2".to_owned()));
    }

    #[test]
    fn test_to_output_args_video_only() {
        // Arrange
        let encode = JlseEncode {
            video: Some(EncodeVideo {
                codec: Some("libx264".to_owned()),
                ..EncodeVideo::default()
            }),
            audio: None,
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — has video mapping but no audio mapping
        assert!(args.contains(&"-map".to_owned()));
        assert!(args.contains(&"0:v".to_owned()));
        assert!(args.contains(&"-c:v".to_owned()));
        assert!(!args.contains(&"0:a".to_owned()));
    }

    #[test]
    fn test_to_output_args_audio_only() {
        // Arrange
        let encode = JlseEncode {
            video: None,
            audio: Some(EncodeAudio {
                codec: Some("aac".to_owned()),
                ..EncodeAudio::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — has audio mapping but no video mapping
        assert!(args.contains(&"0:a".to_owned()));
        assert!(!args.contains(&"0:v".to_owned()));
    }

    #[test]
    fn test_build_encode_args_returns_tuple() {
        // Arrange
        let encode = JlseEncode {
            input: Some(EncodeInput {
                hwaccel: Some("qsv".to_owned()),
                ..EncodeInput::default()
            }),
            video: Some(EncodeVideo {
                codec: Some("hevc_qsv".to_owned()),
                ..EncodeVideo::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let (input_args, output_args) = JlseEncode::build_encode_args(Some(&encode));

        // Assert — input_args contain hwaccel, output_args contain codec
        assert!(input_args.contains(&"-hwaccel".to_owned()));
        assert!(input_args.contains(&"qsv".to_owned()));
        assert!(output_args.contains(&"-c:v".to_owned()));
        assert!(output_args.contains(&"hevc_qsv".to_owned()));
    }

    // ── build_encode_args ────────────────────────────────────

    #[test]
    fn test_build_encode_args_none() {
        // Arrange & Act
        let (input_args, output_args) = JlseEncode::build_encode_args(None);

        // Assert — both empty when no encode config
        assert!(input_args.is_empty());
        assert!(output_args.is_empty());
    }

    #[test]
    fn test_build_encode_args_some_with_video() {
        // Arrange
        let encode = JlseEncode {
            video: Some(EncodeVideo {
                codec: Some("hevc_qsv".to_owned()),
                ..EncodeVideo::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let (input_args, output_args) = JlseEncode::build_encode_args(Some(&encode));

        // Assert — input has global flags, output has video codec
        assert!(input_args.contains(&"-hide_banner".to_owned()));
        assert!(output_args.contains(&"-c:v".to_owned()));
        assert!(output_args.contains(&"hevc_qsv".to_owned()));
    }

    // ── to_output_args edge cases ─────────────────────────────

    #[test]
    fn test_to_output_args_video_no_settings() {
        // Arrange — video section exists but all fields are None/empty
        let encode = JlseEncode {
            video: Some(EncodeVideo {
                codec: None,
                preset: None,
                profile: None,
                pix_fmt: None,
                aspect: None,
                filter: None,
                extra: vec![],
            }),
            audio: None,
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — no -map 0:v because has_settings is false
        assert!(!args.contains(&"-map".to_owned()));
        assert!(args.is_empty());
    }

    #[test]
    fn test_to_output_args_audio_no_settings() {
        // Arrange — audio section exists but all fields are None/empty
        let encode = JlseEncode {
            video: None,
            audio: Some(EncodeAudio {
                codec: None,
                sample_rate: None,
                bitrate: None,
                channels: None,
                extra: vec![],
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — no -map 0:a because has_settings is false
        assert!(!args.contains(&"-map".to_owned()));
        assert!(args.is_empty());
    }

    // ── hwupload auto-prepend ──────────────────────────────

    #[test]
    fn test_to_output_args_hwupload_prepended_and_format_injected() {
        // Arrange — QSV HW encode: filter_hw_device triggers hwupload + format inject
        let encode = JlseEncode {
            input: Some(EncodeInput {
                init_hw_device: Some("qsv=hw".to_owned()),
                filter_hw_device: Some("hw".to_owned()),
                ..EncodeInput::default()
            }),
            video: Some(EncodeVideo {
                codec: Some("av1_qsv".to_owned()),
                filter: Some("vpp_qsv=framerate=30".to_owned()),
                pix_fmt: Some("p010le".to_owned()),
                ..EncodeVideo::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — hwupload prepended, format=p010le injected into VPP
        let vf_idx = args.iter().position(|a| a == "-vf").unwrap();
        assert_eq!(
            args[vf_idx + 1],
            "format=nv12,hwupload=extra_hw_frames=64,vpp_qsv=framerate=30:format=p010le"
        );
    }

    #[test]
    fn test_to_output_args_no_hwupload_without_filter_hw_device() {
        // Arrange — SW encode: no filter_hw_device, filter used as-is
        let encode = JlseEncode {
            input: Some(EncodeInput::default()),
            video: Some(EncodeVideo {
                codec: Some("libx264".to_owned()),
                filter: Some("yadif=mode=send_frame".to_owned()),
                ..EncodeVideo::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — filter unchanged, no hwupload
        let vf_idx = args.iter().position(|a| a == "-vf").unwrap();
        assert_eq!(args[vf_idx + 1], "yadif=mode=send_frame");
    }

    #[test]
    fn test_to_output_args_hwupload_skipped_when_already_at_start() {
        // Arrange — user already specified hwupload at the beginning
        let encode = JlseEncode {
            input: Some(EncodeInput {
                init_hw_device: Some("qsv=hw".to_owned()),
                filter_hw_device: Some("hw".to_owned()),
                ..EncodeInput::default()
            }),
            video: Some(EncodeVideo {
                codec: Some("av1_qsv".to_owned()),
                filter: Some("hwupload=extra_hw_frames=32,vpp_qsv=framerate=30".to_owned()),
                pix_fmt: Some("p010le".to_owned()),
                ..EncodeVideo::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — no duplicate hwupload, format=p010le injected
        let vf_idx = args.iter().position(|a| a == "-vf").unwrap();
        assert_eq!(
            args[vf_idx + 1],
            "hwupload=extra_hw_frames=32,vpp_qsv=framerate=30:format=p010le"
        );
    }

    #[test]
    fn test_to_output_args_hwupload_skipped_when_in_middle() {
        // Arrange — user placed hwupload in the middle of the chain
        let encode = JlseEncode {
            input: Some(EncodeInput {
                init_hw_device: Some("qsv=hw".to_owned()),
                filter_hw_device: Some("hw".to_owned()),
                ..EncodeInput::default()
            }),
            video: Some(EncodeVideo {
                codec: Some("av1_qsv".to_owned()),
                filter: Some(
                    "scale=1280:720,hwupload=extra_hw_frames=64,vpp_qsv=framerate=30".to_owned(),
                ),
                pix_fmt: Some("p010le".to_owned()),
                ..EncodeVideo::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — no duplicate hwupload, format=p010le injected into VPP
        let vf_idx = args.iter().position(|a| a == "-vf").unwrap();
        assert_eq!(
            args[vf_idx + 1],
            "scale=1280:720,hwupload=extra_hw_frames=64,vpp_qsv=framerate=30:format=p010le"
        );
    }

    // ── VPP filter helpers ─────────────────────────────────

    #[test]
    fn test_validate_vpp_no_format_passes_without_format() {
        // Arrange
        let filter = "vpp_qsv=deinterlace=advanced:height=720:width=1280";

        // Act / Assert
        assert!(validate_vpp_no_format(filter).is_ok());
    }

    #[test]
    fn test_validate_vpp_no_format_fails_with_format() {
        // Arrange
        let filter = "vpp_qsv=deinterlace=advanced:format=p010le:height=720";

        // Act
        let err = validate_vpp_no_format(filter).unwrap_err();

        // Assert
        assert!(
            err.to_string().contains("redundant"),
            "error should mention redundancy: {err}"
        );
    }

    #[test]
    fn test_validate_vpp_no_format_passes_for_sw_filter() {
        // Arrange — SW filter, no VPP
        let filter = "yadif=mode=send_frame,scale=1280:720";

        // Act / Assert
        assert!(validate_vpp_no_format(filter).is_ok());
    }

    #[test]
    fn test_inject_vpp_format_basic() {
        // Arrange
        let filter = "vpp_qsv=deinterlace=advanced:height=720:width=1280";

        // Act
        let result = inject_vpp_format(filter, Some("p010le"));

        // Assert
        assert_eq!(
            result,
            "vpp_qsv=deinterlace=advanced:height=720:width=1280:format=p010le"
        );
    }

    #[test]
    fn test_inject_vpp_format_with_setfield() {
        // Arrange — VPP followed by SW filter
        let filter = "vpp_qsv=deinterlace=advanced:height=720:width=1280,setfield=mode=prog";

        // Act
        let result = inject_vpp_format(filter, Some("p010le"));

        // Assert — format injected only into VPP segment
        assert_eq!(
            result,
            "vpp_qsv=deinterlace=advanced:height=720:width=1280:format=p010le,setfield=mode=prog"
        );
    }

    #[test]
    fn test_inject_vpp_format_no_pix_fmt() {
        // Arrange — no pix_fmt
        let filter = "vpp_qsv=deinterlace=advanced:height=720";

        // Act
        let result = inject_vpp_format(filter, None);

        // Assert — unchanged
        assert_eq!(result, "vpp_qsv=deinterlace=advanced:height=720");
    }

    #[test]
    fn test_inject_vpp_format_non_vpp_filter_unchanged() {
        // Arrange — SW filter
        let filter = "yadif=mode=send_frame,scale=1280:720";

        // Act
        let result = inject_vpp_format(filter, Some("p010le"));

        // Assert — SW filters unchanged
        assert_eq!(result, "yadif=mode=send_frame,scale=1280:720");
    }

    #[test]
    fn test_prepare_hw_filter_full_chain() {
        // Arrange
        let filter = "vpp_qsv=deinterlace=advanced:height=720:width=1280,setfield=mode=prog";

        // Act
        let result = prepare_hw_filter(filter, Some("p010le"));

        // Assert — format=nv12 + hwupload prepended, format injected
        assert_eq!(
            result,
            "format=nv12,hwupload=extra_hw_frames=64,vpp_qsv=deinterlace=advanced:height=720:width=1280:format=p010le,setfield=mode=prog"
        );
    }

    // ── stream specifier auto-append ────────────────────────

    #[test]
    fn test_video_extra_global_quality_gets_stream_specifier() {
        // Arrange — av1_qsv ICQ mode with -global_quality
        let encode = JlseEncode {
            video: Some(EncodeVideo {
                codec: Some("av1_qsv".to_owned()),
                extra: vec!["-global_quality".to_owned(), "24".to_owned()],
                ..EncodeVideo::default()
            }),
            audio: Some(EncodeAudio {
                codec: Some("libopus".to_owned()),
                bitrate: Some("128k".to_owned()),
                ..EncodeAudio::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — -global_quality becomes -global_quality:v (not applied to audio)
        assert!(args.contains(&"-global_quality:v".to_owned()));
        assert!(!args.contains(&"-global_quality".to_owned()));
        // Value "24" stays as-is (not a flag)
        assert!(args.contains(&"24".to_owned()));
    }

    #[test]
    fn test_video_extra_movflags_not_modified() {
        // Arrange — -movflags is a muxer-level option, must not get :v
        let encode = JlseEncode {
            video: Some(EncodeVideo {
                codec: Some("libx264".to_owned()),
                extra: vec!["-movflags".to_owned(), "faststart".to_owned()],
                ..EncodeVideo::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert
        assert!(args.contains(&"-movflags".to_owned()));
        assert!(!args.contains(&"-movflags:v".to_owned()));
    }

    #[test]
    fn test_video_extra_preserves_existing_specifier() {
        // Arrange — option already has a stream specifier
        let encode = JlseEncode {
            video: Some(EncodeVideo {
                codec: Some("libx264".to_owned()),
                extra: vec!["-b:v".to_owned(), "5M".to_owned()],
                ..EncodeVideo::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — already has :v, must not become -b:v:v
        assert!(args.contains(&"-b:v".to_owned()));
        assert!(!args.contains(&"-b:v:v".to_owned()));
    }

    #[test]
    fn test_audio_extra_gets_stream_specifier() {
        // Arrange
        let encode = JlseEncode {
            audio: Some(EncodeAudio {
                codec: Some("libopus".to_owned()),
                extra: vec!["-af".to_owned(), "aresample=48000".to_owned()],
                ..EncodeAudio::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — -af becomes -af:a
        assert!(args.contains(&"-af:a".to_owned()));
        assert!(!args.contains(&"-af".to_owned()));
    }

    #[test]
    fn test_all_global_options_excluded_from_specifier() {
        // Arrange — every GLOBAL_OPTIONS entry should pass through unchanged
        let extras: Vec<String> = GLOBAL_OPTIONS.iter().map(|s| (*s).to_owned()).collect();
        let encode = JlseEncode {
            video: Some(EncodeVideo {
                codec: Some("libx264".to_owned()),
                extra: extras.clone(),
                ..EncodeVideo::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_output_args();

        // Assert — none of them should have :v appended
        for opt in &extras {
            assert!(args.contains(opt), "{opt} should be in args unchanged");
            let with_specifier = format!("{opt}:v");
            assert!(
                !args.contains(&with_specifier),
                "{opt} should NOT become {with_specifier}"
            );
        }
    }
}
