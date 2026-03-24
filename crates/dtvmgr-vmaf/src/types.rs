//! Core types for VMAF-based quality parameter search.

use std::fmt;
use std::path::PathBuf;

/// BT.709 color metadata key-value pairs for HD broadcast sources.
///
/// Japanese terrestrial/BS HD broadcasts use BT.709 colour primaries,
/// transfer characteristics, and matrix with limited (TV) range.
pub const BT709_COLOR_ARGS: &[(&str, &str)] = &[
    ("-color_range", "tv"),
    ("-color_primaries", "bt709"),
    ("-color_trc", "bt709"),
    ("-colorspace", "bt709"),
];

/// Pre-formatted BT.709 colour args with `:v` stream specifier.
///
/// Flat `&[&str]` ready to push into an `OsStr` arg list without
/// any runtime allocation.
pub const BT709_COLOR_ARGS_V: &[&str] = &[
    "-color_range:v",
    "tv",
    "-color_primaries:v",
    "bt709",
    "-color_trc:v",
    "bt709",
    "-colorspace:v",
    "bt709",
];

/// Complete configuration for a quality parameter search.
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// Path to the ffmpeg binary.
    pub ffmpeg_bin: PathBuf,
    /// Path to the input TS file.
    pub input_file: PathBuf,
    /// Content segments (main programme intervals, in seconds).
    pub content_segments: Vec<ContentSegment>,
    /// Encoder-specific settings.
    pub encoder: EncoderConfig,
    /// `FFmpeg` video filter chain (e.g. `"yadif=...,scale=1280:720"`).
    pub video_filter: String,
    /// Target VMAF score (default: 93.0).
    pub target_vmaf: f32,
    /// Maximum encoded size as percentage of original (default: 80.0).
    pub max_encoded_percent: f32,
    /// Accepted VMAF shortfall from target (default: 1.0).
    ///
    /// When no quality value achieves `target_vmaf` exactly, a result
    /// within `target_vmaf - min_vmaf_tolerance` is still accepted.
    pub min_vmaf_tolerance: f32,
    /// Strict tolerance mode (reserved for future use).
    pub thorough: bool,
    /// Sample extraction settings.
    pub sample: SampleConfig,
    /// Additional ffmpeg output args (appended after codec/quality args).
    pub extra_encode_args: Vec<String>,
    /// Additional ffmpeg input args (prepended before `-i`).
    pub extra_input_args: Vec<String>,
    /// Video filter for FFV1 reference creation.
    ///
    /// When `None`, [`video_filter`](Self::video_filter) is reused.
    /// Set this when the encode filter outputs HW surface frames
    /// (e.g. QSV VPP) that need `hwdownload,format=…` appended
    /// before the CPU-only FFV1 encoder.
    pub reference_filter: Option<String>,
    /// Temporary directory for intermediate files (uses system default if `None`).
    pub temp_dir: Option<PathBuf>,
}

impl SearchConfig {
    /// Return the effective video filter for FFV1 reference creation.
    ///
    /// Uses [`reference_filter`](Self::reference_filter) when set,
    /// otherwise falls back to [`video_filter`](Self::video_filter).
    #[must_use]
    pub fn effective_reference_filter(&self) -> &str {
        self.reference_filter
            .as_deref()
            .unwrap_or(&self.video_filter)
    }
}

/// A content segment identified by start and end timestamps in seconds.
///
/// Derived from `Trim(start_frame, end_frame)` in `obs_cut.avs`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentSegment {
    /// Segment start in seconds (inclusive).
    pub start_secs: f64,
    /// Segment end in seconds (exclusive).
    pub end_secs: f64,
}

impl ContentSegment {
    /// Duration of this segment in seconds.
    #[must_use]
    pub fn duration(&self) -> f64 {
        self.end_secs - self.start_secs
    }
}

/// Sample extraction settings.
#[derive(Debug, Clone)]
pub struct SampleConfig {
    /// Duration of each sample in seconds (default: 3.0).
    pub duration_secs: f64,
    /// Seconds to skip from the beginning and end of content (default: 120.0).
    pub skip_secs: f64,
    /// Take one sample per this many seconds of content (default: 720.0 = 12 min).
    pub sample_every_secs: f64,
    /// Minimum number of samples to extract (default: 5).
    pub min_samples: u32,
    /// Maximum number of samples to extract (default: 15).
    pub max_samples: u32,
    /// VMAF `n_subsample` — compute score on every Nth frame (default: 5).
    ///
    /// For a 3-second sample at 29.97fps (~90 frames), `n_subsample=5`
    /// means ~18 frames are scored. Set to 1 to score every frame.
    pub vmaf_subsample: u32,
}

impl Default for SampleConfig {
    fn default() -> Self {
        Self {
            duration_secs: 3.0,
            skip_secs: 120.0,
            sample_every_secs: 720.0,
            min_samples: 5,
            max_samples: 15,
            vmaf_subsample: 5,
        }
    }
}

/// Encoder-specific configuration.
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// `FFmpeg` codec name (e.g. `"av1_qsv"`, `"libsvtav1"`).
    pub codec: String,
    /// Quality parameter type.
    pub quality_param: QualityParam,
    /// Minimum quality value (best quality end of range).
    pub min_quality: f32,
    /// Maximum quality value (worst quality end of range).
    pub max_quality: f32,
    /// Quality value step size for the search grid.
    pub quality_increment: f32,
    /// When `true`, higher values mean higher quality (inverted semantics).
    pub high_value_means_hq: bool,
    /// Default ffmpeg output args for this encoder.
    pub default_ffmpeg_args: Vec<String>,
    /// Default ffmpeg input args for this encoder.
    pub default_input_args: Vec<String>,
    /// Encoder speed preset.
    pub preset: Option<String>,
    /// Pixel format override.
    pub pix_fmt: Option<String>,
    /// Initial quality value for the first search iteration.
    ///
    /// Set near the expected VMAF 93 sweet spot (midpoint between
    /// anime and live-action content) to minimise iterations.
    pub quality_hint: f32,
}

/// Quality parameter type — determines the ffmpeg flag name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityParam {
    /// `-crf` (libsvtav1, libx264, libx265, libaom-av1)
    Crf,
    /// `-global_quality` (`av1_qsv`, `hevc_qsv`, `h264_qsv`)
    GlobalQuality,
    /// `-qp` (librav1e, *_vulkan)
    Qp,
    /// `-q` (*_vaapi, mpeg2video)
    Q,
    /// `-cq` (*_nvenc)
    Cq,
}

impl QualityParam {
    /// All quality parameter flag strings.
    pub const ALL_FLAGS: &[&str] = &["-crf", "-global_quality", "-qp", "-q", "-cq"];

    /// All quality parameter flags with `:v` stream specifier.
    pub const ALL_FLAGS_V: &[&str] = &["-crf:v", "-global_quality:v", "-qp:v", "-q:v", "-cq:v"];

    /// Return the ffmpeg flag string for this parameter type.
    #[must_use]
    pub const fn flag(&self) -> &'static str {
        match self {
            Self::Crf => "-crf",
            Self::GlobalQuality => "-global_quality",
            Self::Qp => "-qp",
            Self::Q => "-q",
            Self::Cq => "-cq",
        }
    }
}

impl fmt::Display for QualityParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.flag())
    }
}

/// Result of a quality parameter search.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Optimal quality value found.
    pub quality_value: f32,
    /// Quality parameter flag (e.g. `"-crf"`, `"-global_quality"`).
    pub quality_param: String,
    /// Achieved mean VMAF score at the optimal value.
    pub mean_vmaf: f32,
    /// Predicted encoded size as percentage of original.
    pub predicted_size_percent: f64,
    /// Number of search iterations performed.
    pub iterations: u32,
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    // ── ContentSegment ──────────────────────────────────────

    #[test]
    fn content_segment_duration_normal() {
        // Arrange
        let seg = ContentSegment {
            start_secs: 10.0,
            end_secs: 130.0,
        };

        // Act
        let dur = seg.duration();

        // Assert
        assert!((dur - 120.0).abs() < f64::EPSILON);
    }

    #[test]
    fn content_segment_duration_zero() {
        // Arrange
        let seg = ContentSegment {
            start_secs: 42.0,
            end_secs: 42.0,
        };

        // Act
        let dur = seg.duration();

        // Assert
        assert!(dur.abs() < f64::EPSILON);
    }

    // ── SampleConfig ────────────────────────────────────────

    #[test]
    fn sample_config_default_values() {
        // Arrange / Act
        let cfg = SampleConfig::default();

        // Assert
        assert!((cfg.duration_secs - 3.0).abs() < f64::EPSILON);
        assert!((cfg.skip_secs - 120.0).abs() < f64::EPSILON);
        assert!((cfg.sample_every_secs - 720.0).abs() < f64::EPSILON);
        assert_eq!(cfg.min_samples, 5);
        assert_eq!(cfg.max_samples, 15);
        assert_eq!(cfg.vmaf_subsample, 5);
    }

    // ── SearchConfig ─────────────────────────────────────────

    #[test]
    fn effective_reference_filter_uses_reference_when_set() {
        // Arrange
        let config = SearchConfig {
            ffmpeg_bin: PathBuf::new(),
            input_file: PathBuf::new(),
            content_segments: Vec::new(),
            encoder: crate::EncoderConfig::libx264(),
            video_filter: String::from("yadif,scale=1280:720"),
            target_vmaf: 93.0,
            max_encoded_percent: 80.0,
            min_vmaf_tolerance: 1.0,
            thorough: false,
            sample: SampleConfig::default(),
            extra_encode_args: Vec::new(),
            extra_input_args: Vec::new(),
            reference_filter: Some(String::from("vpp_qsv=...,hwdownload")),
            temp_dir: None,
        };

        // Act / Assert
        assert_eq!(
            config.effective_reference_filter(),
            "vpp_qsv=...,hwdownload"
        );
    }

    #[test]
    fn effective_reference_filter_falls_back_to_video_filter() {
        // Arrange
        let config = SearchConfig {
            ffmpeg_bin: PathBuf::new(),
            input_file: PathBuf::new(),
            content_segments: Vec::new(),
            encoder: crate::EncoderConfig::libx264(),
            video_filter: String::from("yadif,scale=1280:720"),
            target_vmaf: 93.0,
            max_encoded_percent: 80.0,
            min_vmaf_tolerance: 1.0,
            thorough: false,
            sample: SampleConfig::default(),
            extra_encode_args: Vec::new(),
            extra_input_args: Vec::new(),
            reference_filter: None,
            temp_dir: None,
        };

        // Act / Assert
        assert_eq!(config.effective_reference_filter(), "yadif,scale=1280:720");
    }

    // ── QualityParam ────────────────────────────────────────

    #[test]
    fn quality_param_flag_all_variants() {
        // Arrange / Act / Assert
        assert_eq!(QualityParam::Crf.flag(), "-crf");
        assert_eq!(QualityParam::GlobalQuality.flag(), "-global_quality");
        assert_eq!(QualityParam::Qp.flag(), "-qp");
        assert_eq!(QualityParam::Q.flag(), "-q");
        assert_eq!(QualityParam::Cq.flag(), "-cq");
    }

    #[test]
    fn quality_param_all_flags_length_and_contents() {
        // Arrange
        let all_variants = [
            QualityParam::Crf,
            QualityParam::GlobalQuality,
            QualityParam::Qp,
            QualityParam::Q,
            QualityParam::Cq,
        ];

        // Act / Assert — length matches
        assert_eq!(QualityParam::ALL_FLAGS.len(), all_variants.len());

        // Assert — each variant's flag() is in ALL_FLAGS at the same index
        for (i, variant) in all_variants.iter().enumerate() {
            let actual = QualityParam::ALL_FLAGS.get(i).unwrap();
            assert_eq!(
                *actual,
                variant.flag(),
                "ALL_FLAGS[{i}] should match {variant:?}.flag()"
            );
        }
    }

    #[test]
    fn quality_param_display_outputs_flag() {
        // Arrange
        let variants = [
            (QualityParam::Crf, "-crf"),
            (QualityParam::GlobalQuality, "-global_quality"),
            (QualityParam::Qp, "-qp"),
            (QualityParam::Q, "-q"),
            (QualityParam::Cq, "-cq"),
        ];

        // Act / Assert
        for (variant, expected) in variants {
            assert_eq!(format!("{variant}"), expected);
        }
    }
}

/// Progress events emitted during the search.
#[derive(Debug, Clone)]
pub enum SearchProgress {
    /// Extracting samples from the input.
    SampleExtract {
        /// Current sample being extracted (1-indexed).
        current: u32,
        /// Total samples to extract.
        total: u32,
    },
    /// Encoding a sample at a candidate quality value.
    Encoding {
        /// Search iteration number (1-indexed).
        iteration: u32,
        /// Quality value being tested.
        quality: f32,
        /// Current sample being encoded (1-indexed).
        sample: u32,
        /// Total samples.
        total: u32,
    },
    /// Measuring VMAF score for an encoded sample.
    Scoring {
        /// Search iteration number.
        iteration: u32,
        /// Quality value being tested.
        quality: f32,
        /// Current sample being scored (1-indexed).
        sample: u32,
        /// Total samples.
        total: u32,
    },
    /// Result of one search iteration.
    IterationResult {
        /// Iteration number.
        iteration: u32,
        /// Quality value tested.
        quality: f32,
        /// Mean VMAF score across all samples.
        vmaf: f32,
        /// Encoded size as percentage of sample size.
        size_percent: f64,
    },
}
