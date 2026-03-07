//! EPGStation-compatible progress output for the pipeline.
//!
//! When `--epgstation` mode is active, emits JSON progress lines to stdout
//! that `EPGStation` can parse for its encoding progress UI.

use std::fmt::Write as _;

/// Progress output mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::module_name_repetitions)]
pub enum ProgressMode {
    /// `EPGStation`-compatible JSON progress output.
    EpgStation,
}

/// Emit `EPGStation`-compatible progress JSON to stdout.
///
/// Output format: `{"type":"progress","percent":<f64>,"log":"<string>"}`
///
/// This intentionally uses `println!` instead of `tracing` because
/// `EPGStation` reads structured JSON from the child process's stdout.
#[allow(clippy::print_stdout)]
pub fn emit_epgstation(percent: f64, log: &str) {
    let escaped = log.replace('\\', "\\\\").replace('"', "\\\"");
    println!("{{\"type\":\"progress\",\"percent\":{percent:.4},\"log\":\"{escaped}\"}}");
}

/// Parsed `FFmpeg` progress information.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::module_name_repetitions)]
pub struct FfmpegProgress {
    /// Progress percentage (0.0 to 1.0).
    pub percent: f64,
    /// Human-readable log line.
    pub log: String,
}

/// Parse an `FFmpeg` stderr progress line and compute completion percentage.
///
/// `FFmpeg` outputs lines like:
/// ```text
/// frame=  120 fps= 30 ... time=00:00:04.00 ... speed=1.5x
/// ```
///
/// Returns `None` if the line does not contain a `time=` field.
#[must_use]
#[allow(clippy::module_name_repetitions)]
pub fn parse_ffmpeg_progress(line: &str, duration: f64) -> Option<FfmpegProgress> {
    if duration <= 0.0 {
        return None;
    }

    let time_str = extract_field(line, "time=")?;
    let current = time_to_seconds(time_str)?;
    let percent = (current / duration).clamp(0.0, 1.0);

    // Build a compact log from available fields.
    let mut log = String::new();
    if let Some(fps) = extract_field(line, "fps=") {
        let _ = write!(log, "fps={fps}");
    }
    if !log.is_empty() {
        log.push(' ');
    }
    let _ = write!(log, "time={time_str}");
    if let Some(speed) = extract_field(line, "speed=") {
        let _ = write!(log, " speed={speed}");
    }

    Some(FfmpegProgress { percent, log })
}

/// Extract a field value from an `FFmpeg` progress line.
///
/// Fields are whitespace-separated `key=value` pairs.
fn extract_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let idx = line.find(key)?;
    let start = idx.checked_add(key.len())?;
    let after = line.get(start..)?;
    let value = after.trim_start();
    let end = value.find(char::is_whitespace).unwrap_or(value.len());
    let result = value.get(..end)?;
    if result.is_empty() {
        return None;
    }
    Some(result)
}

/// Convert an `HH:MM:SS.ss` or `HH:MM:SS` time string to seconds.
#[allow(clippy::arithmetic_side_effects)]
fn time_to_seconds(time: &str) -> Option<f64> {
    let parts: Vec<&str> = time.splitn(3, ':').collect();
    let h = parts.first().and_then(|s| s.trim().parse::<f64>().ok());
    let m = parts.get(1).and_then(|s| s.trim().parse::<f64>().ok());
    let s = parts.get(2).and_then(|s| s.trim().parse::<f64>().ok());
    match (h, m, s) {
        (Some(hours), Some(minutes), Some(seconds)) => {
            Some(hours.mul_add(3600.0, minutes.mul_add(60.0, seconds)))
        }
        _ => None,
    }
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::suboptimal_flops)]

    use super::*;

    // ── time_to_seconds ─────────────────────────────────────

    #[test]
    fn test_time_to_seconds_zero() {
        assert!((time_to_seconds("00:00:00.00").unwrap()).abs() < f64::EPSILON);
    }

    #[test]
    fn test_time_to_seconds_with_fraction() {
        let secs = time_to_seconds("01:23:45.67").unwrap();
        let expected = 3600.0 + 23.0 * 60.0 + 45.67;
        assert!((secs - expected).abs() < 0.001);
    }

    #[test]
    fn test_time_to_seconds_no_fraction() {
        let secs = time_to_seconds("00:30:00").unwrap();
        assert!((secs - 1800.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_time_to_seconds_invalid() {
        assert!(time_to_seconds("invalid").is_none());
        assert!(time_to_seconds("00:00").is_none());
    }

    // ── extract_field ───────────────────────────────────────

    #[test]
    fn test_extract_field_basic() {
        let line = "frame=  120 fps= 30.0 time=00:00:04.00 speed=1.5x";
        assert_eq!(extract_field(line, "fps="), Some("30.0"));
        assert_eq!(extract_field(line, "time="), Some("00:00:04.00"));
        assert_eq!(extract_field(line, "speed="), Some("1.5x"));
    }

    #[test]
    fn test_extract_field_missing() {
        let line = "frame=  120 fps= 30.0";
        assert_eq!(extract_field(line, "speed="), None);
    }

    // ── parse_ffmpeg_progress ───────────────────────────────

    #[test]
    fn test_parse_ffmpeg_progress_normal() {
        // Arrange
        let line = "frame=  120 fps= 30.0 time=00:05:00.00 speed=2.0x";
        let duration = 600.0; // 10 minutes

        // Act
        let progress = parse_ffmpeg_progress(line, duration).unwrap();

        // Assert
        assert!((progress.percent - 0.5).abs() < 0.001);
        assert!(progress.log.contains("fps=30.0"));
        assert!(progress.log.contains("time=00:05:00.00"));
        assert!(progress.log.contains("speed=2.0x"));
    }

    #[test]
    fn test_parse_ffmpeg_progress_complete() {
        // Arrange
        let line = "frame= 1800 fps= 30 time=00:01:00.00 speed=1x";
        let duration = 60.0;

        // Act
        let progress = parse_ffmpeg_progress(line, duration).unwrap();

        // Assert
        assert!((progress.percent - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_ffmpeg_progress_over_duration_clamped() {
        // Arrange
        let line = "frame= 3600 fps= 60 time=00:01:05.00 speed=1x";
        let duration = 60.0;

        // Act
        let progress = parse_ffmpeg_progress(line, duration).unwrap();

        // Assert — clamped to 1.0
        assert!((progress.percent - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_ffmpeg_progress_no_time_field() {
        let line = "frame=  120 fps= 30.0";
        assert!(parse_ffmpeg_progress(line, 600.0).is_none());
    }

    #[test]
    fn test_parse_ffmpeg_progress_zero_duration() {
        let line = "frame=  120 fps= 30.0 time=00:00:04.00 speed=1.5x";
        assert!(parse_ffmpeg_progress(line, 0.0).is_none());
    }

    #[test]
    fn test_parse_ffmpeg_progress_negative_duration() {
        let line = "frame=  120 fps= 30.0 time=00:00:04.00 speed=1.5x";
        assert!(parse_ffmpeg_progress(line, -1.0).is_none());
    }

    // ── emit_epgstation ─────────────────────────────────────

    #[test]
    fn test_emit_format() {
        // Verify the output format is correct by checking the escaped string.
        let log = r#"test "quoted" value"#;
        let escaped = log.replace('\\', "\\\\").replace('"', "\\\"");
        let output = format!(
            "{{\"type\":\"progress\",\"percent\":{:.4},\"log\":\"{}\"}}",
            0.5, escaped
        );
        assert_eq!(
            output,
            r#"{"type":"progress","percent":0.5000,"log":"test \"quoted\" value"}"#
        );
    }
}
