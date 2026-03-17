//! `FFmpeg` `filter_complex` generation from AVS Trim segments.
//!
//! Reads `obs_cut.avs` Trim commands and generates an `FFmpeg`
//! `-filter_complex` file for frame-accurate cutting.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use tracing::{debug, instrument};

use crate::command::ffprobe::FrameRate;
use crate::output::chapter::TRIM_RE;

// ── Types ────────────────────────────────────────────────────

/// A single Trim segment with start and end frame numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrimSegment {
    /// Start frame (inclusive).
    pub start: u32,
    /// End frame (inclusive).
    pub end: u32,
}

/// Minimum start frame — values below this are clamped.
const MIN_START_FRAME: u32 = 30;

// ── Functions ────────────────────────────────────────────────

/// Parse `Trim(start,end)` commands into [`TrimSegment`] pairs.
///
/// Start frames below [`MIN_START_FRAME`] are clamped to
/// `MIN_START_FRAME`.
///
/// # Panics
///
/// Panics if a captured digit group cannot be parsed as `u32`
/// (should not happen since the regex only captures `\d+`).
#[allow(clippy::expect_used)]
#[must_use]
pub fn parse_trim_segments(content: &str) -> Vec<TrimSegment> {
    let mut segments = Vec::new();

    for cap in TRIM_RE.captures_iter(content) {
        let raw_start: u32 = cap[1].parse().expect("trim start is numeric");
        let end: u32 = cap[2].parse().expect("trim end is numeric");
        let start = raw_start.max(MIN_START_FRAME);
        segments.push(TrimSegment { start, end });
    }

    segments
}

/// Convert a frame number to seconds at the given frame rate.
///
/// Formula: `frame * denominator / numerator`
#[must_use]
pub fn frame_to_time(frame: u32, fps: &FrameRate) -> f64 {
    f64::from(frame) * f64::from(fps.denominator) / f64::from(fps.numerator)
}

/// Generate an ffmpeg `filter_complex` string from Trim segments.
///
/// Each segment produces a `trim=start:end,setpts=PTS-STARTPTS` and
/// `atrim=start:end,asetpts=PTS-STARTPTS` pair. All segments are
/// concatenated via the `concat` filter.
#[must_use]
pub fn generate_filter(segments: &[TrimSegment], fps: &FrameRate) -> String {
    if segments.is_empty() {
        return String::new();
    }

    let mut filter = String::new();

    for (i, seg) in segments.iter().enumerate() {
        let start = frame_to_time(seg.start, fps);
        let end = frame_to_time(seg.end, fps);

        let _ = writeln!(
            filter,
            "[0:v]trim=start={start:.3}:end={end:.3},setpts=PTS-STARTPTS[v{i}];",
        );
        let _ = writeln!(
            filter,
            "[0:a]atrim=start={start:.3}:end={end:.3},asetpts=PTS-STARTPTS[a{i}];",
        );
    }

    let n = segments.len();
    for i in 0..n {
        let _ = write!(filter, "[v{i}][a{i}]");
    }
    let _ = write!(filter, "concat=n={n}:v=1:a=1[outv][outa]");

    filter
}

/// Read an AVS cut file, generate the filter, and write to output.
///
/// # Errors
///
/// Returns an error if the AVS file cannot be read, the filter
/// cannot be generated, or the output file cannot be written.
#[instrument(skip_all, err(level = "error"))]
pub fn create(avs_cut_path: &Path, output_path: &Path, fps: &FrameRate) -> Result<()> {
    let content = std::fs::read_to_string(avs_cut_path)
        .with_context(|| format!("failed to read AVS cut file: {}", avs_cut_path.display()))?;

    let segments = parse_trim_segments(&content);
    let filter = generate_filter(&segments, fps);

    std::fs::write(output_path, &filter)
        .with_context(|| format!("failed to write filter file: {}", output_path.display()))?;

    debug!(path = %output_path.display(), segments = segments.len(), "wrote ffmpeg filter");
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    // ── parse_trim_segments ─────────────────────────────────

    #[test]
    fn test_parse_trim_segments_single() {
        // Arrange / Act
        let result = parse_trim_segments("Trim(100,500)");

        // Assert
        assert_eq!(
            result,
            vec![TrimSegment {
                start: 100,
                end: 500
            }]
        );
    }

    #[test]
    fn test_parse_trim_segments_multiple() {
        // Arrange / Act
        let result = parse_trim_segments("Trim(100,500)Trim(800,1200)");

        // Assert
        assert_eq!(
            result,
            vec![
                TrimSegment {
                    start: 100,
                    end: 500
                },
                TrimSegment {
                    start: 800,
                    end: 1200
                },
            ]
        );
    }

    #[test]
    fn test_parse_trim_segments_clamp_below_min() {
        // Arrange / Act
        let result = parse_trim_segments("Trim(0,500)");

        // Assert — start clamped from 0 to 30
        assert_eq!(
            result,
            vec![TrimSegment {
                start: 30,
                end: 500
            }]
        );
    }

    #[test]
    fn test_parse_trim_segments_clamp_boundary() {
        // Arrange / Act
        let result = parse_trim_segments("Trim(29,500)");

        // Assert — 29 < 30, clamped to 30
        assert_eq!(
            result,
            vec![TrimSegment {
                start: 30,
                end: 500
            }]
        );
    }

    #[test]
    fn test_parse_trim_segments_exact_min() {
        // Arrange / Act
        let result = parse_trim_segments("Trim(30,500)");

        // Assert — 30 == MIN_START_FRAME, no clamp
        assert_eq!(
            result,
            vec![TrimSegment {
                start: 30,
                end: 500
            }]
        );
    }

    #[test]
    fn test_parse_trim_segments_empty() {
        // Arrange / Act / Assert
        assert!(parse_trim_segments("").is_empty());
    }

    #[test]
    fn test_parse_trim_segments_no_match() {
        // Arrange / Act / Assert
        assert!(parse_trim_segments("LWLibavVideoSource(TSFilePath)").is_empty());
    }

    // ── frame_to_time ───────────────────────────────────────

    #[test]
    fn test_frame_to_time_30fps() {
        // Arrange
        let fps = FrameRate {
            numerator: 30,
            denominator: 1,
        };

        // Act
        let time = frame_to_time(90, &fps);

        // Assert — 90 * 1 / 30 = 3.0
        assert!((time - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_frame_to_time_29_97fps() {
        // Arrange
        let fps = FrameRate {
            numerator: 30000,
            denominator: 1001,
        };

        // Act
        let time = frame_to_time(900, &fps);

        // Assert — 900 * 1001 / 30000 = 30.03
        let expected = 900.0 * 1001.0 / 30000.0;
        assert!((time - expected).abs() < 1e-9);
    }

    #[test]
    fn test_frame_to_time_zero() {
        // Arrange
        let fps = FrameRate {
            numerator: 30,
            denominator: 1,
        };

        // Act / Assert
        assert!((frame_to_time(0, &fps)).abs() < f64::EPSILON);
    }

    // ── generate_filter ─────────────────────────────────────

    #[test]
    fn test_generate_filter_single_segment() {
        // Arrange
        let fps = FrameRate {
            numerator: 30,
            denominator: 1,
        };
        let segments = vec![TrimSegment {
            start: 30,
            end: 900,
        }];

        // Act
        let result = generate_filter(&segments, &fps);

        // Assert
        assert!(result.contains("trim=start=1.000:end=30.000"));
        assert!(result.contains("atrim=start=1.000:end=30.000"));
        assert!(result.contains("[v0][a0]concat=n=1:v=1:a=1[outv][outa]"));
    }

    #[test]
    fn test_generate_filter_two_segments() {
        // Arrange
        let fps = FrameRate {
            numerator: 30,
            denominator: 1,
        };
        let segments = vec![
            TrimSegment {
                start: 30,
                end: 900,
            },
            TrimSegment {
                start: 1800,
                end: 2700,
            },
        ];

        // Act
        let result = generate_filter(&segments, &fps);

        // Assert
        assert!(result.contains("[v0]"));
        assert!(result.contains("[a0]"));
        assert!(result.contains("[v1]"));
        assert!(result.contains("[a1]"));
        assert!(result.contains("[v0][a0][v1][a1]concat=n=2:v=1:a=1[outv][outa]"));
    }

    #[test]
    fn test_generate_filter_empty() {
        // Arrange
        let fps = FrameRate {
            numerator: 30,
            denominator: 1,
        };

        // Act / Assert
        assert!(generate_filter(&[], &fps).is_empty());
    }

    // ── create ──────────────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_create_writes_filter_file() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let avs_path = tmp.path().join("obs_cut.avs");
        let filter_path = tmp.path().join("ffmpeg.filter");
        let fps = FrameRate {
            numerator: 30000,
            denominator: 1001,
        };
        std::fs::write(&avs_path, "Trim(100,500)\nTrim(800,1200)").unwrap();

        // Act
        create(&avs_path, &filter_path, &fps).unwrap();

        // Assert
        let content = std::fs::read_to_string(&filter_path).unwrap();
        assert!(content.contains("trim="));
        assert!(content.contains("concat=n=2"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_create_empty_avs() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let avs_path = tmp.path().join("obs_cut.avs");
        let filter_path = tmp.path().join("ffmpeg.filter");
        let fps = FrameRate {
            numerator: 30,
            denominator: 1,
        };
        std::fs::write(&avs_path, "LWLibavVideoSource(TSFilePath)").unwrap();

        // Act
        create(&avs_path, &filter_path, &fps).unwrap();

        // Assert
        let content = std::fs::read_to_string(&filter_path).unwrap();
        assert!(content.is_empty());
    }
}
