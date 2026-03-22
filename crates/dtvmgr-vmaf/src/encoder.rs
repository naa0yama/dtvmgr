//! Encoder-specific configuration and quality-space conversion.
//!
//! Provides [`EncoderConfig`] presets for common encoders and the
//! [`QualityConverter`] that maps continuous quality values (CRF / ICQ)
//! to a discrete integer search space used by the binary search.

use crate::types::{EncoderConfig, QualityParam};

// ── Encoder presets ──────────────────────────────────────────

/// Default QSV ffmpeg args (look-ahead + external bitrate control).
fn qsv_default_args() -> Vec<String> {
    vec![
        String::from("-look_ahead"),
        String::from("1"),
        String::from("-extbrc"),
        String::from("1"),
        String::from("-look_ahead_depth"),
        String::from("40"),
    ]
}

impl EncoderConfig {
    /// Intel QSV AV1 hardware encoder preset.
    ///
    /// ICQ 18–35, hint 25. Range estimated from h264/hevc QSV trends.
    #[must_use]
    pub fn av1_qsv() -> Self {
        Self {
            codec: String::from("av1_qsv"),
            quality_param: QualityParam::GlobalQuality,
            min_quality: 18.0,
            max_quality: 35.0,
            quality_increment: 1.0,
            high_value_means_hq: false,
            default_ffmpeg_args: qsv_default_args(),
            default_input_args: Vec::new(),
            preset: None,
            pix_fmt: None,
            quality_hint: 25.0,
        }
    }

    /// SVT-AV1 software encoder preset.
    ///
    /// CRF 25–45, hint 35. Nature reaches VMAF 93 at CRF 31,
    /// anime at CRF ~38. Default preset 8.
    #[must_use]
    pub fn libsvtav1() -> Self {
        Self {
            codec: String::from("libsvtav1"),
            quality_param: QualityParam::Crf,
            min_quality: 25.0,
            max_quality: 45.0,
            quality_increment: 1.0,
            high_value_means_hq: false,
            default_ffmpeg_args: Vec::new(),
            default_input_args: Vec::new(),
            preset: Some(String::from("8")),
            pix_fmt: Some(String::from("yuv420p10le")),
            quality_hint: 35.0,
        }
    }

    /// Intel QSV H.264 hardware encoder preset.
    ///
    /// ICQ 20–32, hint 27. Nature reaches VMAF 93 at GQ 25,
    /// anime at GQ 28–29.
    #[must_use]
    pub fn h264_qsv() -> Self {
        Self {
            codec: String::from("h264_qsv"),
            quality_param: QualityParam::GlobalQuality,
            min_quality: 20.0,
            max_quality: 32.0,
            quality_increment: 1.0,
            high_value_means_hq: false,
            default_ffmpeg_args: qsv_default_args(),
            default_input_args: Vec::new(),
            preset: None,
            pix_fmt: None,
            quality_hint: 27.0,
        }
    }

    /// Intel QSV HEVC hardware encoder preset.
    ///
    /// ICQ 18–28, hint 23. Nature reaches VMAF 93 at GQ 20,
    /// anime at GQ 27.
    #[must_use]
    pub fn hevc_qsv() -> Self {
        Self {
            codec: String::from("hevc_qsv"),
            quality_param: QualityParam::GlobalQuality,
            min_quality: 18.0,
            max_quality: 28.0,
            quality_increment: 1.0,
            high_value_means_hq: false,
            default_ffmpeg_args: qsv_default_args(),
            default_input_args: Vec::new(),
            preset: None,
            pix_fmt: None,
            quality_hint: 23.0,
        }
    }

    /// x264 software encoder preset.
    ///
    /// CRF 20–30, hint 25. Nature reaches VMAF 93 at CRF 23,
    /// anime at CRF 26–27.
    #[must_use]
    pub fn libx264() -> Self {
        Self {
            codec: String::from("libx264"),
            quality_param: QualityParam::Crf,
            min_quality: 20.0,
            max_quality: 30.0,
            quality_increment: 1.0,
            high_value_means_hq: false,
            default_ffmpeg_args: Vec::new(),
            default_input_args: Vec::new(),
            preset: Some(String::from("medium")),
            pix_fmt: Some(String::from("yuv420p")),
            quality_hint: 25.0,
        }
    }

    /// x265 software encoder preset.
    ///
    /// CRF 20–32, hint 25. Nature reaches VMAF 93 at CRF 23,
    /// anime at CRF 27.
    #[must_use]
    pub fn libx265() -> Self {
        Self {
            codec: String::from("libx265"),
            quality_param: QualityParam::Crf,
            min_quality: 20.0,
            max_quality: 32.0,
            quality_increment: 1.0,
            high_value_means_hq: false,
            default_ffmpeg_args: Vec::new(),
            default_input_args: Vec::new(),
            preset: Some(String::from("medium")),
            pix_fmt: Some(String::from("yuv420p10le")),
            quality_hint: 25.0,
        }
    }
}

// ── Quality-space converter ──────────────────────────────────

/// Maps continuous quality values to a discrete integer search space.
///
/// The binary search operates on integer `q` values to avoid
/// floating-point comparison issues.  The converter normalises all
/// encoders so that **lower `q` = higher quality**.
#[derive(Debug, Clone, Copy)]
pub(crate) struct QualityConverter {
    /// Step size between adjacent quality values.
    increment: f32,
    /// When `true`, higher raw values mean higher quality (inverted).
    high_means_hq: bool,
}

impl QualityConverter {
    /// Create a converter from encoder configuration.
    pub(crate) const fn new(config: &EncoderConfig) -> Self {
        Self {
            increment: config.quality_increment,
            high_means_hq: config.high_value_means_hq,
        }
    }

    /// Convert a quality value (CRF/ICQ) to an integer `q`.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        clippy::arithmetic_side_effects
    )]
    pub(crate) fn q(self, quality: f32) -> i64 {
        let q = (f64::from(quality) / f64::from(self.increment)).round() as i64;
        if self.high_means_hq { -q } else { q }
    }

    /// Convert an integer `q` back to a quality value.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        clippy::arithmetic_side_effects
    )]
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn quality(self, q: i64) -> f32 {
        let pos_q = if self.high_means_hq { -q } else { q };
        (pos_q as f64 * f64::from(self.increment)) as f32
    }

    /// Return `(min_q, max_q)` from quality bounds.
    ///
    /// The returned range always satisfies `min_q < max_q` where
    /// `min_q` corresponds to the best-quality end.
    pub(crate) fn min_max_q(self, min_quality: f32, max_quality: f32) -> (i64, i64) {
        if self.high_means_hq {
            (self.q(max_quality), self.q(min_quality))
        } else {
            (self.q(min_quality), self.q(max_quality))
        }
    }
}

// ── VMAF linear interpolation ────────────────────────────────

/// A search attempt with its quality setting and VMAF result.
#[derive(Debug, Clone)]
pub(crate) struct SearchSample {
    /// Integer quality-space value.
    pub(crate) q: i64,
    /// Continuous quality value (CRF / ICQ).
    pub(crate) quality: f32,
    /// Mean VMAF score across all samples at this quality.
    pub(crate) vmaf: f32,
    /// Encoded size as percentage of sample size.
    pub(crate) size_percent: f64,
}

/// Linearly interpolate in VMAF-space to estimate the `q` value that
/// would achieve `target_vmaf`.
///
/// # Preconditions
///
/// - `worse.vmaf < target_vmaf < better.vmaf`
/// - `worse.q > better.q` (worse quality = higher q)
///
/// The result is clamped to `[better.q + 1, worse.q - 1]` to guarantee
/// progress (at least one step away from either bound).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::as_conversions,
    clippy::arithmetic_side_effects
)]
pub(crate) fn vmaf_lerp_q(target_vmaf: f32, worse: &SearchSample, better: &SearchSample) -> i64 {
    let vmaf_diff = better.vmaf - worse.vmaf;
    let vmaf_factor = (target_vmaf - worse.vmaf) / vmaf_diff;

    let q_diff = worse.q - better.q;
    let lerp = (q_diff as f32)
        .mul_add(-vmaf_factor, worse.q as f32)
        .round() as i64;

    // When worse and better are adjacent (q_diff == 1), there is no
    // integer between them — return better.q (higher quality side).
    if worse.q - better.q <= 1 {
        return better.q;
    }

    lerp.clamp(better.q + 1, worse.q - 1)
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    // ── QualityConverter ─────────────────────────────────────

    #[test]
    fn quality_converter_round_trip_libsvtav1() {
        // Arrange
        let enc = EncoderConfig::libsvtav1();
        let conv = QualityConverter::new(&enc);

        // Act / Assert — round-trip through q and back (integer CRF)
        for &crf in &[25.0_f32, 30.0, 35.0, 45.0] {
            let q = conv.q(crf);
            let back = conv.quality(q);
            assert!(
                (back - crf).abs() < enc.quality_increment,
                "round-trip failed for crf={crf}: got {back}"
            );
        }
    }

    #[test]
    fn quality_converter_lower_q_means_better_quality() {
        // Arrange
        let conv = QualityConverter::new(&EncoderConfig::av1_qsv());

        // Act
        let q_good = conv.q(18.0); // low ICQ = good
        let q_bad = conv.q(35.0); // high ICQ = bad

        // Assert
        assert!(q_good < q_bad, "lower q should mean higher quality");
    }

    #[test]
    fn quality_converter_min_max_q_ordering() {
        // Arrange
        let enc = EncoderConfig::libx264();
        let conv = QualityConverter::new(&enc);

        // Act
        let (min_q, max_q) = conv.min_max_q(enc.min_quality, enc.max_quality);

        // Assert
        assert!(min_q < max_q);
    }

    // ── vmaf_lerp_q ──────────────────────────────────────────

    #[test]
    fn vmaf_lerp_q_adjacent_q_values() {
        // Arrange — q_diff == 1, should return better.q without panic
        let worse = SearchSample {
            q: 101,
            quality: 25.25,
            vmaf: 90.0,
            size_percent: 50.0,
        };
        let better = SearchSample {
            q: 100,
            quality: 25.0,
            vmaf: 98.0,
            size_percent: 80.0,
        };

        // Act
        let q = vmaf_lerp_q(94.0, &worse, &better);

        // Assert — adjacent shortcut returns better.q
        assert_eq!(q, better.q);
    }

    #[test]
    fn vmaf_lerp_q_same_q_values() {
        // Arrange — q_diff == 0, edge case
        let worse = SearchSample {
            q: 100,
            quality: 25.0,
            vmaf: 90.0,
            size_percent: 50.0,
        };
        let better = SearchSample {
            q: 100,
            quality: 25.0,
            vmaf: 98.0,
            size_percent: 80.0,
        };

        // Act
        let q = vmaf_lerp_q(94.0, &worse, &better);

        // Assert — returns better.q (q_diff <= 1 shortcut)
        assert_eq!(q, better.q);
    }

    // ── QualityConverter with high_means_hq ─────────────────

    #[test]
    fn quality_converter_high_means_hq_negates_q() {
        // Arrange — create a config with high_value_means_hq = true
        let mut enc = EncoderConfig::libx264();
        enc.high_value_means_hq = true;
        let conv = QualityConverter::new(&enc);

        // Act — q() should negate
        let q = conv.q(25.0);

        // Assert
        assert_eq!(q, -25);
    }

    #[test]
    fn quality_converter_high_means_hq_quality_un_negates() {
        // Arrange
        let mut enc = EncoderConfig::libx264();
        enc.high_value_means_hq = true;
        let conv = QualityConverter::new(&enc);

        // Act — quality() should un-negate
        let quality = conv.quality(-25);

        // Assert
        assert!((quality - 25.0).abs() < f32::EPSILON);
    }

    #[test]
    fn quality_converter_high_means_hq_min_max_q_swaps() {
        // Arrange
        let mut enc = EncoderConfig::libx264();
        enc.high_value_means_hq = true;
        enc.min_quality = 20.0;
        enc.max_quality = 30.0;
        let conv = QualityConverter::new(&enc);

        // Act
        let (min_q, max_q) = conv.min_max_q(enc.min_quality, enc.max_quality);

        // Assert — should swap: q(max_quality) < q(min_quality)
        assert!(min_q < max_q, "min_q={min_q} should be < max_q={max_q}");
        // min_q = q(30) = -30, max_q = q(20) = -20
        assert_eq!(min_q, -30);
        assert_eq!(max_q, -20);
    }

    #[test]
    fn vmaf_lerp_q_midpoint() {
        // Arrange — target exactly between two points
        let worse = SearchSample {
            q: 200,
            quality: 50.0,
            vmaf: 90.0,
            size_percent: 50.0,
        };
        let better = SearchSample {
            q: 100,
            quality: 25.0,
            vmaf: 98.0,
            size_percent: 80.0,
        };

        // Act
        let q = vmaf_lerp_q(94.0, &worse, &better);

        // Assert — should be near midpoint (150)
        assert!(q > better.q);
        assert!(q < worse.q);
        assert_eq!(q, 150);
    }

    #[test]
    fn vmaf_lerp_q_clamped_to_bounds() {
        // Arrange — target very close to better, but result must stay ≥ better.q + 1
        let worse = SearchSample {
            q: 102,
            quality: 25.5,
            vmaf: 90.0,
            size_percent: 50.0,
        };
        let better = SearchSample {
            q: 100,
            quality: 25.0,
            vmaf: 98.0,
            size_percent: 80.0,
        };

        // Act
        let q = vmaf_lerp_q(97.9, &worse, &better);

        // Assert — clamped to better.q + 1
        assert_eq!(q, 101);
    }

    #[test]
    fn vmaf_lerp_q_clamped_upper() {
        // Arrange — target very close to worse
        let worse = SearchSample {
            q: 200,
            quality: 50.0,
            vmaf: 90.0,
            size_percent: 50.0,
        };
        let better = SearchSample {
            q: 100,
            quality: 25.0,
            vmaf: 98.0,
            size_percent: 80.0,
        };

        // Act
        let q = vmaf_lerp_q(90.1, &worse, &better);

        // Assert — clamped to worse.q - 1
        assert_eq!(q, 199);
    }

    // ── Encoder presets ──────────────────────────────────────

    #[test]
    fn av1_qsv_uses_global_quality() {
        let enc = EncoderConfig::av1_qsv();
        assert_eq!(enc.quality_param, QualityParam::GlobalQuality);
        assert_eq!(enc.quality_param.flag(), "-global_quality");
        assert!(!enc.high_value_means_hq);
    }

    #[test]
    fn libsvtav1_defaults() {
        let enc = EncoderConfig::libsvtav1();
        assert_eq!(enc.quality_param, QualityParam::Crf);
        assert!((enc.quality_increment - 1.0).abs() < f32::EPSILON);
        assert!((enc.min_quality - 25.0).abs() < f32::EPSILON);
        assert!((enc.max_quality - 45.0).abs() < f32::EPSILON);
        assert!((enc.quality_hint - 35.0).abs() < f32::EPSILON);
        assert_eq!(enc.preset.as_deref(), Some("8"));
        assert_eq!(enc.pix_fmt.as_deref(), Some("yuv420p10le"));
    }

    #[test]
    fn h264_qsv_defaults() {
        let enc = EncoderConfig::h264_qsv();
        assert_eq!(enc.quality_param, QualityParam::GlobalQuality);
        assert!((enc.quality_hint - 27.0).abs() < f32::EPSILON);
        assert!(!enc.default_ffmpeg_args.is_empty());
    }

    #[test]
    fn hevc_qsv_defaults() {
        let enc = EncoderConfig::hevc_qsv();
        assert_eq!(enc.quality_param, QualityParam::GlobalQuality);
        assert!((enc.quality_hint - 23.0).abs() < f32::EPSILON);
    }

    #[test]
    fn libx264_integer_step() {
        let enc = EncoderConfig::libx264();
        assert!((enc.quality_increment - 1.0).abs() < f32::EPSILON);
        assert!((enc.quality_hint - 25.0).abs() < f32::EPSILON);
    }

    #[test]
    fn libx265_integer_step() {
        let enc = EncoderConfig::libx265();
        assert!((enc.quality_increment - 1.0).abs() < f32::EPSILON);
        assert!((enc.quality_hint - 25.0).abs() < f32::EPSILON);
    }
}
