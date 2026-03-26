//! Wrapper for the `ffprobe` external command.
//!
//! Extracts frame rate, sample rate, stream durations, and audio stream
//! metadata from media files.

use std::ffi::OsStr;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tracing::instrument;

/// Audio stream metadata from ffprobe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioStreamInfo {
    /// Stream index in the container (0-based, across all stream types).
    pub index: u32,
    /// Number of channels (1=mono, 2=stereo, 6=5.1).
    pub channels: u32,
    /// Channel layout string (e.g. `"stereo"`, `"mono"`, `"5.1"`).
    pub channel_layout: String,
    /// Codec name (e.g. `"aac"`).
    pub codec: String,
}

/// Video frame rate as a numerator/denominator pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRate {
    /// Numerator (e.g. 30000).
    pub numerator: u32,
    /// Denominator (e.g. 1001).
    pub denominator: u32,
}

/// Query the video frame rate of `input_file` using `ffprobe`.
///
/// # Errors
///
/// Returns an error if `ffprobe` fails or the output cannot be parsed.
#[instrument(skip_all, err(level = "error"))]
pub fn frame_rate(binary: &Path, input_file: &Path) -> Result<FrameRate> {
    let args = build_frame_rate_args(input_file);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let stdout = super::run_capture(binary, &os_args)
        .with_context(|| "failed to get frame rate via ffprobe")?;
    parse_frame_rate(stdout.trim())
}

/// Query the audio sample rate of `input_file` using `ffprobe`.
///
/// Returns `Ok(None)` if `ffprobe` produces empty output (no audio stream).
///
/// # Errors
///
/// Returns an error if `ffprobe` fails or the output cannot be parsed as `u32`.
#[instrument(skip_all, err(level = "error"))]
pub fn sample_rate(binary: &Path, input_file: &Path) -> Result<Option<u32>> {
    let args = build_sample_rate_args(input_file);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let stdout = super::run_capture(binary, &os_args)
        .with_context(|| "failed to get sample rate via ffprobe")?;

    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let rate: u32 = trimmed
        .parse()
        .with_context(|| format!("invalid sample rate: {trimmed:?}"))?;
    Ok(Some(rate))
}

/// Parse a frame rate string like `"30000/1001"` into a [`FrameRate`].
///
/// # Errors
///
/// Returns an error if the string is not in `"num/den"` format or if
/// the parts cannot be parsed as `u32`.
pub fn parse_frame_rate(s: &str) -> Result<FrameRate> {
    let (num_str, den_str) = s
        .split_once('/')
        .with_context(|| format!("invalid frame rate format: {s:?}"))?;

    let numerator: u32 = num_str
        .trim()
        .parse()
        .with_context(|| format!("invalid frame rate numerator: {num_str:?}"))?;

    let denominator: u32 = den_str
        .trim()
        .parse()
        .with_context(|| format!("invalid frame rate denominator: {den_str:?}"))?;

    if denominator == 0 {
        bail!("frame rate denominator must not be zero");
    }

    Ok(FrameRate {
        numerator,
        denominator,
    })
}

/// Check whether a specific stream exists in `input_file`.
///
/// Queries the `codec_name` property; returns `true` if the stream
/// is present (non-empty output), `false` otherwise.
///
/// # Errors
///
/// Returns an error if `ffprobe` itself fails to run.
#[instrument(skip_all, err(level = "error"))]
pub fn stream_exists(binary: &Path, input_file: &Path, stream: &str) -> Result<bool> {
    let args = build_probe_args(input_file, stream, "stream=codec_name");
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let stdout = super::run_capture(binary, &os_args)
        .with_context(|| format!("failed to check stream existence for {stream}"))?;
    Ok(!stdout.trim().is_empty())
}

/// Query all audio streams from `input_file` using ffprobe JSON output.
///
/// Returns an empty `Vec` if no audio streams exist.
///
/// # Errors
///
/// Returns an error if `ffprobe` fails or the JSON output cannot be parsed.
#[instrument(skip_all, err(level = "error"))]
pub fn audio_streams(binary: &Path, input_file: &Path) -> Result<Vec<AudioStreamInfo>> {
    let args = vec![
        "-v".to_owned(),
        "error".to_owned(),
        "-select_streams".to_owned(),
        "a".to_owned(),
        "-show_entries".to_owned(),
        "stream=index,channels,channel_layout,codec_name".to_owned(),
        "-of".to_owned(),
        "json".to_owned(),
        input_file.display().to_string(),
    ];
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let stdout = super::run_capture(binary, &os_args)
        .with_context(|| "failed to get audio streams via ffprobe")?;

    parse_audio_streams(&stdout)
}

/// JSON structure returned by ffprobe for stream queries.
#[derive(Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
}

/// A single stream entry in ffprobe JSON output.
#[derive(Deserialize)]
struct FfprobeStream {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    channels: u32,
    #[serde(default)]
    channel_layout: String,
    #[serde(default)]
    codec_name: String,
}

/// Parse ffprobe JSON output into a list of [`AudioStreamInfo`].
fn parse_audio_streams(json_str: &str) -> Result<Vec<AudioStreamInfo>> {
    let trimmed = json_str.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let output: FfprobeOutput =
        serde_json::from_str(trimmed).with_context(|| "failed to parse ffprobe JSON output")?;

    Ok(output
        .streams
        .into_iter()
        .map(|s| AudioStreamInfo {
            index: s.index,
            channels: s.channels,
            channel_layout: s.channel_layout,
            codec: s.codec_name,
        })
        .collect())
}

/// Query the duration of a specific stream in `input_file` using `ffprobe`.
///
/// Returns `Ok(None)` if the stream does not exist or reports `N/A`.
///
/// # Errors
///
/// Returns an error if `ffprobe` fails or the output cannot be parsed as `f64`.
#[instrument(skip_all, err(level = "error"))]
pub fn stream_duration(binary: &Path, input_file: &Path, stream: &str) -> Result<Option<f64>> {
    let args = build_probe_args(input_file, stream, "stream=duration");
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let stdout = super::run_capture(binary, &os_args)
        .with_context(|| format!("failed to get stream duration for {stream}"))?;

    let trimmed = stdout.trim();
    if trimmed.is_empty() || trimmed == "N/A" {
        return Ok(None);
    }

    let dur: f64 = trimmed
        .parse()
        .with_context(|| format!("invalid stream duration: {trimmed:?}"))?;
    Ok(Some(dur))
}

/// Query the total duration of `input_file` in seconds using `ffprobe`.
///
/// # Errors
///
/// Returns an error if `ffprobe` fails or the output cannot be parsed as `f64`.
#[instrument(skip_all, err(level = "error"))]
pub fn duration(binary: &Path, input_file: &Path) -> Result<f64> {
    let args = build_duration_args(input_file);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let stdout = super::run_capture(binary, &os_args)
        .with_context(|| "failed to get duration via ffprobe")?;

    let trimmed = stdout.trim();
    trimmed
        .parse::<f64>()
        .with_context(|| format!("invalid duration value: {trimmed:?}"))
}

/// Build ffprobe args for a single stream property query.
fn build_probe_args(input_file: &Path, stream: &str, entry: &str) -> Vec<String> {
    vec![
        "-v".to_owned(),
        "error".to_owned(),
        "-select_streams".to_owned(),
        stream.to_owned(),
        "-show_entries".to_owned(),
        entry.to_owned(),
        "-of".to_owned(),
        "default=noprint_wrappers=1:nokey=1".to_owned(),
        input_file.display().to_string(),
    ]
}

/// Build args to query video frame rate.
fn build_frame_rate_args(input_file: &Path) -> Vec<String> {
    build_probe_args(input_file, "v:0", "stream=r_frame_rate")
}

/// Build args to query audio sample rate.
fn build_sample_rate_args(input_file: &Path) -> Vec<String> {
    build_probe_args(input_file, "a:0", "stream=sample_rate")
}

/// Build args to query format duration.
fn build_duration_args(input_file: &Path) -> Vec<String> {
    vec![
        "-v".to_owned(),
        "error".to_owned(),
        "-show_entries".to_owned(),
        "format=duration".to_owned(),
        "-of".to_owned(),
        "default=noprint_wrappers=1:nokey=1".to_owned(),
        input_file.display().to_string(),
    ]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn test_parse_frame_rate_normal() {
        // Arrange / Act
        let rate = parse_frame_rate("30000/1001").unwrap();

        // Assert
        assert_eq!(
            rate,
            FrameRate {
                numerator: 30000,
                denominator: 1001
            }
        );
    }

    #[test]
    fn test_parse_frame_rate_simple() {
        // Arrange / Act
        let rate = parse_frame_rate("30/1").unwrap();

        // Assert
        assert_eq!(
            rate,
            FrameRate {
                numerator: 30,
                denominator: 1
            }
        );
    }

    #[test]
    fn test_parse_frame_rate_24000_1001() {
        // Arrange / Act
        let rate = parse_frame_rate("24000/1001").unwrap();

        // Assert
        assert_eq!(
            rate,
            FrameRate {
                numerator: 24000,
                denominator: 1001
            }
        );
    }

    #[test]
    fn test_parse_frame_rate_invalid_no_slash() {
        // Arrange / Act / Assert
        assert!(parse_frame_rate("30").is_err());
    }

    #[test]
    fn test_parse_frame_rate_invalid_non_numeric() {
        // Arrange / Act / Assert
        assert!(parse_frame_rate("abc/def").is_err());
    }

    #[test]
    fn test_parse_frame_rate_zero_denominator() {
        // Arrange / Act / Assert
        assert!(parse_frame_rate("30/0").is_err());
    }

    #[test]
    fn test_parse_frame_rate_empty() {
        // Arrange / Act / Assert
        assert!(parse_frame_rate("").is_err());
    }

    #[test]
    fn test_build_frame_rate_args() {
        // Arrange
        let input = Path::new("/rec/video.ts");

        // Act
        let args = build_frame_rate_args(input);

        // Assert
        assert_eq!(args[0], "-v");
        assert_eq!(args[1], "error");
        assert_eq!(args[2], "-select_streams");
        assert_eq!(args[3], "v:0");
        assert!(args.contains(&"/rec/video.ts".to_owned()));
    }

    #[test]
    fn test_build_sample_rate_args() {
        // Arrange
        let input = Path::new("/rec/video.ts");

        // Act
        let args = build_sample_rate_args(input);

        // Assert
        assert_eq!(args[3], "a:0");
        assert!(args.contains(&"/rec/video.ts".to_owned()));
    }

    #[test]
    fn test_parse_frame_rate_with_whitespace() {
        // Arrange / Act
        let rate = parse_frame_rate(" 30000 / 1001 ").unwrap();

        // Assert
        assert_eq!(
            rate,
            FrameRate {
                numerator: 30000,
                denominator: 1001
            }
        );
    }

    #[test]
    fn test_parse_frame_rate_60fps() {
        // Arrange / Act
        let rate = parse_frame_rate("60000/1001").unwrap();

        // Assert
        assert_eq!(
            rate,
            FrameRate {
                numerator: 60000,
                denominator: 1001
            }
        );
    }

    #[test]
    fn test_parse_frame_rate_negative() {
        // Arrange / Act / Assert: negative numbers fail u32 parse
        assert!(parse_frame_rate("-1/1").is_err());
    }

    // ── run via write_script ─────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_frame_rate_via_script() {
        // Arrange: script that prints frame rate to stdout
        let dir = tempfile::tempdir().unwrap();
        let script = super::super::test_utils::write_script(
            dir.path(),
            "ffprobe.sh",
            "#!/bin/sh\necho '30000/1001'",
        );
        let input = dir.path().join("video.ts");
        std::fs::write(&input, "dummy").unwrap();

        // Act
        let rate = frame_rate(&script, &input).unwrap();

        // Assert
        assert_eq!(rate.numerator, 30000);
        assert_eq!(rate.denominator, 1001);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_sample_rate_via_script() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = super::super::test_utils::write_script(
            dir.path(),
            "ffprobe.sh",
            "#!/bin/sh\necho '48000'",
        );
        let input = dir.path().join("video.ts");
        std::fs::write(&input, "dummy").unwrap();

        // Act
        let rate = sample_rate(&script, &input).unwrap();

        // Assert
        assert_eq!(rate, Some(48000));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_sample_rate_empty_output() {
        // Arrange: script outputs nothing (no audio stream)
        let dir = tempfile::tempdir().unwrap();
        let script =
            super::super::test_utils::write_script(dir.path(), "ffprobe.sh", "#!/bin/sh\necho ''");
        let input = dir.path().join("video.ts");
        std::fs::write(&input, "dummy").unwrap();

        // Act
        let rate = sample_rate(&script, &input).unwrap();

        // Assert
        assert_eq!(rate, None);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_duration_via_script() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = super::super::test_utils::write_script(
            dir.path(),
            "ffprobe.sh",
            "#!/bin/sh\necho '1800.5'",
        );
        let input = dir.path().join("video.ts");
        std::fs::write(&input, "dummy").unwrap();

        // Act
        let dur = duration(&script, &input).unwrap();

        // Assert
        assert!((dur - 1800.5).abs() < f64::EPSILON);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_frame_rate_failure() {
        // Arrange: script exits with error
        let dir = tempfile::tempdir().unwrap();
        let script =
            super::super::test_utils::write_script(dir.path(), "ffprobe.sh", "#!/bin/sh\nexit 1");
        let input = dir.path().join("video.ts");
        std::fs::write(&input, "dummy").unwrap();

        // Act
        let result = frame_rate(&script, &input);

        // Assert
        assert!(result.is_err());
    }

    // ── stream_exists via write_script ─────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_stream_exists_true() {
        // Arrange: ffprobe returns a codec name → stream exists
        let dir = tempfile::tempdir().unwrap();
        let script = super::super::test_utils::write_script(
            dir.path(),
            "ffprobe.sh",
            "#!/bin/sh\necho 'av1'",
        );
        let input = dir.path().join("video.mkv");
        std::fs::write(&input, "dummy").unwrap();

        // Act / Assert
        assert!(stream_exists(&script, &input, "v:0").unwrap());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_stream_exists_false() {
        // Arrange: ffprobe returns empty → stream does not exist
        let dir = tempfile::tempdir().unwrap();
        let script =
            super::super::test_utils::write_script(dir.path(), "ffprobe.sh", "#!/bin/sh\necho ''");
        let input = dir.path().join("video.mkv");
        std::fs::write(&input, "dummy").unwrap();

        // Act / Assert
        assert!(!stream_exists(&script, &input, "v:0").unwrap());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_stream_exists_failure() {
        // Arrange: ffprobe exits with error
        let dir = tempfile::tempdir().unwrap();
        let script =
            super::super::test_utils::write_script(dir.path(), "ffprobe.sh", "#!/bin/sh\nexit 1");
        let input = dir.path().join("video.mkv");
        std::fs::write(&input, "dummy").unwrap();

        // Act / Assert
        assert!(stream_exists(&script, &input, "v:0").is_err());
    }

    // ── stream_duration via write_script ───────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_stream_duration_some() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = super::super::test_utils::write_script(
            dir.path(),
            "ffprobe.sh",
            "#!/bin/sh\necho '1234.567'",
        );
        let input = dir.path().join("video.mkv");
        std::fs::write(&input, "dummy").unwrap();

        // Act
        let dur = stream_duration(&script, &input, "v:0").unwrap();

        // Assert
        assert!((dur.unwrap() - 1234.567).abs() < f64::EPSILON);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_stream_duration_empty() {
        // Arrange: no stream → empty output
        let dir = tempfile::tempdir().unwrap();
        let script =
            super::super::test_utils::write_script(dir.path(), "ffprobe.sh", "#!/bin/sh\necho ''");
        let input = dir.path().join("video.mkv");
        std::fs::write(&input, "dummy").unwrap();

        // Act
        let dur = stream_duration(&script, &input, "v:0").unwrap();

        // Assert
        assert_eq!(dur, None);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_stream_duration_na() {
        // Arrange: ffprobe reports N/A
        let dir = tempfile::tempdir().unwrap();
        let script = super::super::test_utils::write_script(
            dir.path(),
            "ffprobe.sh",
            "#!/bin/sh\necho 'N/A'",
        );
        let input = dir.path().join("video.mkv");
        std::fs::write(&input, "dummy").unwrap();

        // Act
        let dur = stream_duration(&script, &input, "a:0").unwrap();

        // Assert
        assert_eq!(dur, None);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_stream_duration_failure() {
        // Arrange: ffprobe exits with error
        let dir = tempfile::tempdir().unwrap();
        let script =
            super::super::test_utils::write_script(dir.path(), "ffprobe.sh", "#!/bin/sh\nexit 1");
        let input = dir.path().join("video.mkv");
        std::fs::write(&input, "dummy").unwrap();

        // Act / Assert
        assert!(stream_duration(&script, &input, "v:0").is_err());
    }

    #[test]
    fn test_build_duration_args() {
        // Arrange
        let input = Path::new("/rec/video.ts");

        // Act
        let args = build_duration_args(input);

        // Assert
        assert_eq!(args[0], "-v");
        assert_eq!(args[1], "error");
        assert_eq!(args[2], "-show_entries");
        assert_eq!(args[3], "format=duration");
        assert_eq!(args[4], "-of");
        assert_eq!(args[5], "default=noprint_wrappers=1:nokey=1");
        assert_eq!(args[6], "/rec/video.ts");
    }

    // ── parse_audio_streams ─────────────────────────────

    #[test]
    fn test_parse_audio_streams_stereo_and_secondary() {
        // Arrange: typical Japanese broadcast — main stereo + secondary stereo
        let json = r#"{
            "streams": [
                {"index": 1, "codec_name": "aac", "channels": 2, "channel_layout": "stereo"},
                {"index": 2, "codec_name": "aac", "channels": 2, "channel_layout": "stereo"}
            ]
        }"#;

        // Act
        let streams = parse_audio_streams(json).unwrap();

        // Assert
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].index, 1);
        assert_eq!(streams[0].channels, 2);
        assert_eq!(streams[0].channel_layout, "stereo");
        assert_eq!(streams[1].index, 2);
    }

    #[test]
    fn test_parse_audio_streams_dual_mono() {
        // Arrange: dual mono — two separate mono streams
        let json = r#"{
            "streams": [
                {"index": 1, "codec_name": "aac", "channels": 1, "channel_layout": "mono"},
                {"index": 2, "codec_name": "aac", "channels": 1, "channel_layout": "mono"}
            ]
        }"#;

        // Act
        let streams = parse_audio_streams(json).unwrap();

        // Assert
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].channels, 1);
        assert_eq!(streams[1].channels, 1);
    }

    #[test]
    fn test_parse_audio_streams_single() {
        // Arrange: single audio stream
        let json = r#"{
            "streams": [
                {"index": 1, "codec_name": "aac", "channels": 2, "channel_layout": "stereo"}
            ]
        }"#;

        // Act
        let streams = parse_audio_streams(json).unwrap();

        // Assert
        assert_eq!(streams.len(), 1);
    }

    #[test]
    fn test_parse_audio_streams_empty() {
        // Arrange: no audio streams
        let json = r#"{"streams": []}"#;

        // Act
        let streams = parse_audio_streams(json).unwrap();

        // Assert
        assert!(streams.is_empty());
    }

    #[test]
    fn test_parse_audio_streams_empty_string() {
        // Arrange
        let streams = parse_audio_streams("").unwrap();

        // Assert
        assert!(streams.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_audio_streams_via_script() {
        // Arrange: script outputs ffprobe JSON
        let dir = tempfile::tempdir().unwrap();
        let json_output = r#"{"streams":[{"index":1,"codec_name":"aac","channels":2,"channel_layout":"stereo"},{"index":2,"codec_name":"aac","channels":2,"channel_layout":"stereo"}]}"#;
        let script = super::super::test_utils::write_script(
            dir.path(),
            "ffprobe.sh",
            &format!("#!/bin/sh\necho '{json_output}'"),
        );
        let input = dir.path().join("video.ts");
        std::fs::write(&input, "dummy").unwrap();

        // Act
        let streams = audio_streams(&script, &input).unwrap();

        // Assert
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].codec, "aac");
    }
}
