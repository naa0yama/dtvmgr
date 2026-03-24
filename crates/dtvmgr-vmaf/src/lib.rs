//! VMAF-based quality parameter search for video encoding.
//!
//! This crate implements an interpolated binary search algorithm (inspired
//! by [ab-av1](https://github.com/alexheretic/ab-av1)) that automatically
//! finds the optimal CRF / ICQ value for a given video by:
//!
//! 1. Extracting short samples from the input TS via `-c:v copy`.
//! 2. Encoding each sample at a candidate quality value.
//! 3. Measuring VMAF (upscaled to 1080p for calibrated scoring).
//! 4. Interpolating the next candidate from observed (quality, VMAF) pairs.
//!
//! The caller supplies content segments (derived from CM-cut `Trim()`
//! information) so that only main-programme intervals are sampled.

pub mod encode;
pub mod encoder;
pub mod sample;
pub mod search;
pub mod types;
pub mod vmaf;

pub use types::{
    BT709_COLOR_ARGS, BT709_COLOR_ARGS_V, ContentSegment, EncoderConfig, QualityParam,
    SampleConfig, SearchConfig, SearchProgress, SearchResult,
};

use anyhow::Result;

/// Find the optimal quality parameter value for encoding.
///
/// Extracts samples from the input TS file within the specified content
/// segments, then runs an interpolated binary search over quality values
/// using VMAF scoring to converge on the target quality.
///
/// # Errors
///
/// Returns an error if sample extraction, encoding, or VMAF measurement
/// fails, or if no quality value can achieve the target VMAF within the
/// size constraint.
// NOTEST(external-cmd): delegates to search::run
pub fn find_optimal_quality(
    config: &SearchConfig,
    on_progress: Option<&dyn Fn(SearchProgress)>,
) -> Result<SearchResult> {
    search::run(config, on_progress)
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::sample::test_utils::write_mock_ffmpeg;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn find_optimal_quality_with_mock() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let mock_ffmpeg = write_mock_ffmpeg(dir.path());

        let input_ts = dir.path().join("input.ts");
        std::fs::write(&input_ts, b"fake ts data").unwrap();

        let config = SearchConfig {
            ffmpeg_bin: mock_ffmpeg,
            input_file: input_ts,
            content_segments: vec![ContentSegment {
                start_secs: 0.0,
                end_secs: 600.0,
            }],
            encoder: EncoderConfig::libx264(),
            video_filter: String::from("null"),
            target_vmaf: 93.0,
            max_encoded_percent: 200.0, // generous to ensure convergence
            min_vmaf_tolerance: 2.0,
            thorough: false,
            sample: SampleConfig {
                duration_secs: 3.0,
                skip_secs: 0.0,
                sample_every_secs: 720.0,
                min_samples: 1,
                max_samples: 1,
                vmaf_subsample: 5,
            },
            extra_encode_args: Vec::new(),
            extra_input_args: Vec::new(),
            reference_filter: None,
            temp_dir: Some(dir.path().to_owned()),
        };

        // Act
        let result = find_optimal_quality(&config, None).unwrap();

        // Assert — mock always returns VMAF 94.5 so search should converge
        assert!(
            result.mean_vmaf >= 93.0,
            "expected VMAF >= 93.0, got {}",
            result.mean_vmaf
        );
        assert!(
            result.iterations > 0,
            "expected at least 1 iteration, got {}",
            result.iterations
        );
        assert!(
            !result.quality_param.is_empty(),
            "quality_param should not be empty"
        );
    }
}
