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

/// Progress event emitted by the pipeline.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::module_name_repetitions)]
pub enum ProgressEvent {
    /// A pipeline stage is starting.
    StageStart {
        /// Current stage number (1-indexed).
        stage: u8,
        /// Total number of stages.
        total: u8,
        /// Stage name.
        name: String,
    },
    /// Intra-stage progress update (0.0 to 1.0) with status text.
    StageProgress {
        /// Progress within the current stage.
        percent: f64,
        /// Human-readable status text for the stage display.
        log: String,
    },
    /// `FFmpeg` encoding progress update.
    Encoding {
        /// Progress percentage (0.0 to 1.0).
        percent: f64,
        /// Human-readable log line.
        log: String,
    },
    /// A log line from an external command's stderr.
    Log(String),
    /// Pipeline finished successfully.
    Finished,
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

    // Extract fields once for both ETA calculation and log building.
    let speed_str = extract_field(line, "speed=");
    let speed_value = speed_str
        .and_then(|s| s.trim_end_matches('x').parse::<f64>().ok())
        .filter(|&s| s > 0.0);
    let remaining = duration - current;

    // Build a compact log: "ETA: HH:MM:SS fps=N/s speed=Nx"
    let mut log = String::new();
    if let Some(speed) = speed_value {
        if remaining > 0.0 {
            #[allow(clippy::arithmetic_side_effects)]
            let eta_secs = remaining / speed;
            let total = eta_secs.round().max(0.0);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::as_conversions
            )]
            let total = total as u64;
            let _ = write!(
                log,
                "ETA: {:02}:{:02}:{:02}",
                total / 3600,
                (total % 3600) / 60,
                total % 60
            );
        } else {
            let _ = write!(log, "ETA: 00:00:00");
        }
    }
    if let Some(fps) = extract_field(line, "fps=") {
        if !log.is_empty() {
            log.push(' ');
        }
        let _ = write!(log, "fps={fps}/s");
    }
    if let Some(speed) = speed_str {
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

// ── Stage stderr parsers ─────────────────────────────────────

/// Parse `AviSynth` lwi index creation percentage from stderr.
///
/// Matches lines containing `Creating lwi index file XX%`.
#[must_use]
pub fn parse_lwi_percent(line: &str) -> Option<u32> {
    let marker = "Creating lwi index file ";
    let idx = line.find(marker)?;
    let after = line.get(idx.checked_add(marker.len())?..)?;
    let pct_str = after.split('%').next()?;
    pct_str.trim().parse().ok()
}

/// Parse total video frames from `chapter_exe` stderr.
///
/// Matches lines containing `Video Frames: NNNN`.
#[must_use]
pub fn parse_video_frames_total(line: &str) -> Option<u32> {
    let marker = "Video Frames:";
    let idx = line.find(marker)?;
    let after = line.get(idx.checked_add(marker.len())?..)?;
    let num_str = after.split_whitespace().next()?;
    num_str.parse().ok()
}

/// Parse current frame position from `chapter_exe` mute detection stderr.
///
/// Matches lines like `mute 1: 1234 - 5678フレーム`.
#[must_use]
pub fn parse_mute_frame(line: &str) -> Option<u32> {
    let idx = line.find("mute")?;
    let after_mute = line.get(idx.checked_add(4)?..)?;
    let colon_idx = after_mute.find(':')?;
    let after_colon = after_mute.get(colon_idx.checked_add(1)?..)?;
    let frame_str = after_colon
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()?;
    if frame_str.is_empty() {
        return None;
    }
    frame_str.parse().ok()
}

/// Parse `logoframe` checking progress from stderr.
///
/// Matches lines like `checking 1234/5678 ended.`.
#[must_use]
pub fn parse_logoframe_checking(line: &str) -> Option<(u32, u32)> {
    let marker = "checking";
    let idx = line.find(marker)?;
    let after = line.get(idx.checked_add(marker.len())?..)?;
    let trimmed = after.trim_start();
    let (current_str, rest) = trimmed.split_once('/')?;
    let total_str = rest.split_whitespace().next()?;
    match (
        current_str.trim().parse::<u32>(),
        total_str.trim().parse::<u32>(),
    ) {
        (Ok(current), Ok(total)) => Some((current, total)),
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

        // Assert — 50% done, ETA = 300s / 2.0x = 150s = 00:02:30
        assert!((progress.percent - 0.5).abs() < 0.001);
        assert!(progress.log.contains("ETA: 00:02:30"));
        assert!(progress.log.contains("fps=30.0/s"));
        assert!(progress.log.contains("speed=2.0x"));
        assert!(!progress.log.contains("frame="));
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

    #[test]
    fn test_parse_ffmpeg_progress_no_speed() {
        // Arrange — time present but no speed= field
        let line = "frame=  120 fps= 30.0 time=00:05:00.00";
        let duration = 600.0;

        // Act
        let progress = parse_ffmpeg_progress(line, duration).unwrap();

        // Assert — no ETA since no speed, but fps is present
        assert!((progress.percent - 0.5).abs() < 0.001);
        assert!(progress.log.contains("fps=30.0/s"));
        assert!(!progress.log.contains("ETA:"));
    }

    #[test]
    fn test_parse_ffmpeg_progress_done_remaining_zero() {
        // Arrange — current >= duration, remaining <= 0
        let line = "frame= 3600 fps= 60 time=00:10:00.00 speed=1.0x";
        let duration = 600.0; // 10 minutes

        // Act
        let progress = parse_ffmpeg_progress(line, duration).unwrap();

        // Assert — ETA: 00:00:00 when remaining <= 0
        assert!(progress.log.contains("ETA: 00:00:00"));
    }

    #[test]
    fn test_parse_mute_frame_empty_after_colon() {
        // Arrange — colon present but no digit after it
        assert_eq!(parse_mute_frame("mute 1: "), None);
    }

    #[test]
    fn test_parse_logoframe_checking_invalid_numbers() {
        // Arrange — current/total are not valid u32
        assert_eq!(parse_logoframe_checking("checking abc/def ended."), None);
    }

    #[test]
    fn test_extract_field_empty_value() {
        // Arrange — key present but value is empty (end of string)
        let line = "time=";
        assert_eq!(extract_field(line, "time="), None);
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

    // ── parse_lwi_percent ──────────────────────────────────

    #[test]
    fn test_parse_lwi_percent_basic() {
        assert_eq!(parse_lwi_percent("Creating lwi index file 50%"), Some(50));
    }

    #[test]
    fn test_parse_lwi_percent_with_prefix() {
        assert_eq!(
            parse_lwi_percent("AviSynth Creating lwi index file 75%"),
            Some(75)
        );
    }

    #[test]
    fn test_parse_lwi_percent_100() {
        assert_eq!(parse_lwi_percent("Creating lwi index file 100%"), Some(100));
    }

    #[test]
    fn test_parse_lwi_percent_no_match() {
        assert_eq!(parse_lwi_percent("some other output"), None);
    }

    // ── parse_video_frames_total ───────────────────────────

    #[test]
    fn test_parse_video_frames_total() {
        assert_eq!(
            parse_video_frames_total("\tVideo Frames: 12345 [29.97fps]"),
            Some(12345)
        );
    }

    #[test]
    fn test_parse_video_frames_total_with_prefix() {
        assert_eq!(
            parse_video_frames_total("chapter_exe \tVideo Frames: 6789 [29.97fps]"),
            Some(6789)
        );
    }

    #[test]
    fn test_parse_video_frames_total_no_match() {
        assert_eq!(parse_video_frames_total("some other output"), None);
    }

    // ── parse_mute_frame ───────────────────────────────────

    #[test]
    fn test_parse_mute_frame_basic() {
        assert_eq!(parse_mute_frame("mute 1: 1234 - 5678フレーム"), Some(1234));
    }

    #[test]
    fn test_parse_mute_frame_no_space() {
        assert_eq!(parse_mute_frame("mute0: 500 - 1000フレーム"), Some(500));
    }

    #[test]
    fn test_parse_mute_frame_no_match() {
        assert_eq!(parse_mute_frame("some other output"), None);
    }

    // ── parse_logoframe_checking ───────────────────────────

    #[test]
    fn test_parse_logoframe_checking_basic() {
        assert_eq!(
            parse_logoframe_checking("checking 1234/5678 ended."),
            Some((1234, 5678))
        );
    }

    #[test]
    fn test_parse_logoframe_checking_with_prefix() {
        assert_eq!(
            parse_logoframe_checking("logoframe checking 100/200 ended."),
            Some((100, 200))
        );
    }

    #[test]
    fn test_parse_logoframe_checking_no_match() {
        assert_eq!(parse_logoframe_checking("some other output"), None);
    }
}
