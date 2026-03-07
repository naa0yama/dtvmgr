//! Wrapper for the `ffmpeg` external command.
//!
//! Encodes media files into MKV with optional chapter metadata and
//! EIT XML attachment.

use std::ffi::OsStr;
use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use tracing::debug;

use crate::progress::{self, ProgressEvent};
use crate::types::JlseEncode;

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
pub fn run(
    binary: &Path,
    avs_file: &Path,
    output_file: &Path,
    chapter_file: Option<&Path>,
    metadata: &MkvMetadata,
    extra_options: &str,
) -> Result<()> {
    let args = build_args(avs_file, output_file, chapter_file, metadata, extra_options);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    super::run(binary, &os_args)
}

/// Build the argument list for ffmpeg (MKV output).
///
/// Format:
/// ```text
/// ffmpeg -y -i <avs_file>
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
    extra_options: &str,
) -> Vec<String> {
    let mut args = vec![
        "-y".to_owned(),
        "-i".to_owned(),
        avs_file.display().to_string(),
    ];

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
    /// Convert encode configuration to `FFmpeg` argument list.
    ///
    /// Generated order:
    /// 1. Input flags (`-fflags`, `-analyzeduration`, `-probesize`)
    /// 2. Video mapping (`-map 0:v`)
    /// 3. Video codec / preset / profile / `pix_fmt` / filter / extra
    /// 4. Audio mapping (`-map 0:a`)
    /// 5. Audio codec / `sample_rate` / bitrate / channels / extra
    #[must_use]
    #[allow(clippy::cognitive_complexity)]
    pub fn to_ffmpeg_args(&self) -> Vec<String> {
        let mut args = vec!["-hide_banner".to_owned(), "-ignore_unknown".to_owned()];

        // Input flags
        if let Some(ref input) = self.input {
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
        }

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
                args.push(filter.clone());
            }
            args.extend(video.extra.iter().cloned());
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
                args.push("-ab".to_owned());
                args.push(bitrate.clone());
            }
            if let Some(channels) = audio.channels {
                args.push("-ac".to_owned());
                args.push(channels.to_string());
            }
            args.extend(audio.extra.iter().cloned());
        }

        args
    }

    /// Build encode args from an optional TOML config and optional CLI extra options.
    ///
    /// Merges `to_ffmpeg_args()` output with whitespace-split CLI options.
    #[must_use]
    pub fn build_encode_args(encode: Option<&Self>, cli_opts: Option<&str>) -> Vec<String> {
        let mut args = encode.map_or_else(Vec::new, Self::to_ffmpeg_args);
        if let Some(opts) = cli_opts {
            args.extend(opts.split_whitespace().map(String::from));
        }
        args
    }
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
#[allow(clippy::too_many_arguments)]
pub fn run_with_progress(
    binary: &Path,
    avs_file: &Path,
    output_file: &Path,
    chapter_file: Option<&Path>,
    metadata: &MkvMetadata,
    extra_options: &str,
    duration: f64,
    on_progress: &dyn Fn(ProgressEvent),
) -> Result<()> {
    let args = build_args(avs_file, output_file, chapter_file, metadata, extra_options);
    debug!(cmd = %binary.display(), ?args, "running ffmpeg with progress");

    let mut child = Command::new(binary)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
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
        );

        // Assert
        assert!(!args.iter().any(|a| a.contains("movflags")));
    }

    // ── to_ffmpeg_args ─────────────────────────────────────

    #[test]
    fn test_to_ffmpeg_args_empty() {
        // Arrange
        let encode = JlseEncode::default();

        // Act
        let args = encode.to_ffmpeg_args();

        // Assert — only global flags
        assert_eq!(args, vec!["-hide_banner", "-ignore_unknown"]);
    }

    #[test]
    fn test_to_ffmpeg_args_full() {
        // Arrange
        let encode = JlseEncode {
            format: Some("mkv".to_owned()),
            input: Some(EncodeInput {
                flags: Some("+discardcorrupt+genpts".to_owned()),
                analyzeduration: Some("30M".to_owned()),
                probesize: Some("100M".to_owned()),
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
                    "-color_range".to_owned(),
                    "tv".to_owned(),
                    "-color_primaries".to_owned(),
                    "bt709".to_owned(),
                    "-color_trc".to_owned(),
                    "bt709".to_owned(),
                    "-colorspace".to_owned(),
                    "bt709".to_owned(),
                    "-max_muxing_queue_size".to_owned(),
                    "4000".to_owned(),
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
        };

        // Act
        let args = encode.to_ffmpeg_args();

        // Assert — global flags
        assert_eq!(args[0], "-hide_banner");
        assert_eq!(args[1], "-ignore_unknown");

        // Assert — input flags
        assert_eq!(args[2], "-fflags");
        assert_eq!(args[3], "+discardcorrupt+genpts");
        assert_eq!(args[4], "-analyzeduration");
        assert_eq!(args[5], "30M");
        assert_eq!(args[6], "-probesize");
        assert_eq!(args[7], "100M");

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
        assert!(args.contains(&"-crf".to_owned()));
        assert!(args.contains(&"23".to_owned()));
        assert!(args.contains(&"-color_range".to_owned()));
        assert!(args.contains(&"tv".to_owned()));
        assert!(args.contains(&"-colorspace".to_owned()));
        assert!(args.contains(&"bt709".to_owned()));
        assert!(args.contains(&"-max_muxing_queue_size".to_owned()));
        assert!(args.contains(&"4000".to_owned()));
        assert!(args.contains(&"-movflags".to_owned()));
        assert!(args.contains(&"faststart".to_owned()));

        // Assert — audio mapping and codec
        assert!(args.contains(&"0:a".to_owned()));
        assert!(args.contains(&"-c:a".to_owned()));
        assert!(args.contains(&"aac".to_owned()));
        assert!(args.contains(&"-ar".to_owned()));
        assert!(args.contains(&"48000".to_owned()));
        assert!(args.contains(&"-ab".to_owned()));
        assert!(args.contains(&"256k".to_owned()));
        assert!(args.contains(&"-ac".to_owned()));
        assert!(args.contains(&"2".to_owned()));
    }

    #[test]
    fn test_to_ffmpeg_args_video_only() {
        // Arrange
        let encode = JlseEncode {
            video: Some(EncodeVideo {
                codec: Some("libx264".to_owned()),
                ..EncodeVideo::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_ffmpeg_args();

        // Assert — has video mapping but no audio mapping
        assert!(args.contains(&"-map".to_owned()));
        assert!(args.contains(&"0:v".to_owned()));
        assert!(args.contains(&"-c:v".to_owned()));
        assert!(!args.contains(&"0:a".to_owned()));
    }

    #[test]
    fn test_to_ffmpeg_args_audio_only() {
        // Arrange
        let encode = JlseEncode {
            audio: Some(EncodeAudio {
                codec: Some("aac".to_owned()),
                ..EncodeAudio::default()
            }),
            ..JlseEncode::default()
        };

        // Act
        let args = encode.to_ffmpeg_args();

        // Assert — has audio mapping but no video mapping
        assert!(args.contains(&"0:a".to_owned()));
        assert!(!args.contains(&"0:v".to_owned()));
    }
}
