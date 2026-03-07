//! Wrapper for the `ffprobe` external command.
//!
//! Extracts frame rate and sample rate from media files.

use std::ffi::OsStr;
use std::path::Path;

use anyhow::{Context, Result, bail};

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
pub fn get_frame_rate(binary: &Path, input_file: &Path) -> Result<FrameRate> {
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
pub fn get_sample_rate(binary: &Path, input_file: &Path) -> Result<Option<u32>> {
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

/// Query the total duration of `input_file` in seconds using `ffprobe`.
///
/// # Errors
///
/// Returns an error if `ffprobe` fails or the output cannot be parsed as `f64`.
pub fn get_duration(binary: &Path, input_file: &Path) -> Result<f64> {
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
}
