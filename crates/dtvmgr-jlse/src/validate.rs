//! Pre-encode duration validation.
//!
//! Compares the original TS duration against the CM-cut AVS duration
//! to detect cut errors before encoding begins.

use std::path::Path;

use anyhow::{Context, Result, bail};
use tracing::info;

use crate::command::ffprobe;
use crate::types::DurationCheckRule;

/// Default duration rules based on the JS reference implementation.
///
/// | Program length | Min percent |
/// |----------------|-------------|
/// | ≤ 10 min       | 68%         |
/// | 11–49 min      | 75%         |
/// | 50–90 min      | 70%         |
/// | ≥ 91 min       | 70%         |
pub const DEFAULT_RULES: &[DurationCheckRule] = &[
    DurationCheckRule {
        min_min: 0,
        max_min: 10,
        min_percent: 68,
    },
    DurationCheckRule {
        min_min: 11,
        max_min: 49,
        min_percent: 75,
    },
    DurationCheckRule {
        min_min: 50,
        max_min: 90,
        min_percent: 70,
    },
    DurationCheckRule {
        min_min: 91,
        max_min: 9999,
        min_percent: 70,
    },
];

/// Validate that the AVS-to-TS duration ratio meets the threshold.
///
/// Converts rule minutes to seconds and percent to ratio internally.
/// If no rule matches the TS duration, the check passes.
///
/// # Errors
///
/// Returns an error if the TS duration is zero or if the ratio is
/// below the threshold for the matching rule.
#[allow(clippy::module_name_repetitions)]
pub fn validate_duration_ratio(
    ts_duration_secs: f64,
    avs_duration_secs: f64,
    rules: &[DurationCheckRule],
) -> Result<()> {
    if ts_duration_secs <= 0.0 {
        bail!("TS duration is zero or negative: {ts_duration_secs}s");
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    let (ratio_percent, ts_minutes) = {
        let pct = (avs_duration_secs / ts_duration_secs * 100.0).floor() as u32;
        let min = (ts_duration_secs / 60.0).round() as u32;
        (pct, min)
    };

    info!(
        ts_secs = ts_duration_secs,
        avs_secs = avs_duration_secs,
        ts_min = ts_minutes,
        ratio_percent,
        "duration check"
    );

    for rule in rules {
        if ts_minutes >= rule.min_min && ts_minutes <= rule.max_min {
            if ratio_percent <= rule.min_percent {
                bail!(
                    "content ratio {ratio_percent}% is below threshold {min}% \
                     for {ts_min}min ({ts_secs:.0}s) program (avs={avs_secs:.0}s)",
                    min = rule.min_percent,
                    ts_min = ts_minutes,
                    ts_secs = ts_duration_secs,
                    avs_secs = avs_duration_secs,
                );
            }
            return Ok(());
        }
    }

    // No rule matched — check passes.
    Ok(())
}

/// Run pre-encode duration validation using ffprobe.
///
/// Queries durations of the original TS and CM-cut AVS files,
/// then validates the ratio against the provided rules (or defaults).
/// Returns the AVS duration so callers can reuse it (e.g. for progress).
///
/// # Errors
///
/// Returns an error if ffprobe fails or the duration ratio is too low.
pub fn check_pre_encode_duration(
    ffprobe_bin: &Path,
    ts_file: &Path,
    avs_file: &Path,
    rules: Option<&[DurationCheckRule]>,
) -> Result<f64> {
    let ts_duration = ffprobe::get_duration(ffprobe_bin, ts_file)
        .with_context(|| format!("failed to get TS duration: {}", ts_file.display()))?;
    let avs_duration = ffprobe::get_duration(ffprobe_bin, avs_file)
        .with_context(|| format!("failed to get AVS duration: {}", avs_file.display()))?;

    let effective_rules = rules.unwrap_or(DEFAULT_RULES);
    validate_duration_ratio(ts_duration, avs_duration, effective_rules)
        .context("duration ratio validation failed")?;
    Ok(avs_duration)
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    // ── Short program (≤ 10min) ──────────────────────────────

    #[test]
    fn short_program_acceptable_ratio() {
        // Arrange: 10min program, 80% content
        let ts = 600.0;
        let avs = 480.0;

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_ok());
    }

    #[test]
    fn short_program_too_low_ratio() {
        // Arrange: 10min program, 60% content (≤ 68%)
        let ts = 600.0;
        let avs = 360.0;

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_err());
    }

    #[test]
    fn short_program_boundary_68_percent_fails() {
        // Arrange: 10min program, exactly 68% (≤ 68% fails)
        let ts = 600.0;
        let avs = 408.0; // 408/600 = 68%

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_err());
    }

    #[test]
    fn short_program_boundary_69_percent_passes() {
        // Arrange: 10min program, 69% content
        let ts = 600.0;
        let avs = 414.0; // 414/600 = 69%

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_ok());
    }

    // ── Medium program (11–49min) ────────────────────────────

    #[test]
    fn medium_program_acceptable_ratio() {
        // Arrange: 30min program, 85% content
        let ts = 1800.0;
        let avs = 1530.0;

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_ok());
    }

    #[test]
    fn medium_program_too_low_ratio() {
        // Arrange: 30min program, 70% content (≤ 75%)
        let ts = 1800.0;
        let avs = 1260.0;

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_err());
    }

    // ── Long program (50–90min) ──────────────────────────────

    #[test]
    fn long_program_acceptable_ratio() {
        // Arrange: 60min program, 80% content
        let ts = 3600.0;
        let avs = 2880.0;

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_ok());
    }

    #[test]
    fn long_program_too_low_ratio() {
        // Arrange: 60min program, 65% content (≤ 70%)
        let ts = 3600.0;
        let avs = 2340.0;

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_err());
    }

    // ── Very long program (≥ 91min) ──────────────────────────

    #[test]
    fn very_long_program_acceptable_ratio() {
        // Arrange: 120min program, 80% content
        let ts = 7200.0;
        let avs = 5760.0;

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_ok());
    }

    #[test]
    fn very_long_program_too_low_ratio() {
        // Arrange: 120min program, 65% content (≤ 70%)
        let ts = 7200.0;
        let avs = 4680.0;

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_err());
    }

    // ── Edge cases ───────────────────────────────────────────

    #[test]
    fn zero_ts_duration_fails() {
        // Arrange / Act / Assert
        assert!(validate_duration_ratio(0.0, 100.0, DEFAULT_RULES).is_err());
    }

    #[test]
    fn negative_ts_duration_fails() {
        // Arrange / Act / Assert
        assert!(validate_duration_ratio(-1.0, 100.0, DEFAULT_RULES).is_err());
    }

    #[test]
    fn boundary_600s_uses_short_rule() {
        // Arrange: exactly 600s = 10min, should use ≤10min rule (68%)
        let ts = 600.0;
        let avs = 414.0; // 69%

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_ok());
    }

    #[test]
    fn boundary_660s_uses_medium_rule() {
        // Arrange: 660s = 11min, should use 11–49min rule (75%)
        let ts = 660.0;
        let avs = 502.0; // 502/660 = 76.06% → floor = 76%

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_ok());
    }

    #[test]
    fn boundary_660s_below_medium_threshold() {
        // Arrange: 660s = 11min, 74% content (≤ 75%)
        let ts = 660.0;
        let avs = 488.0; // 488/660 = 73.9% → floor = 73%

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, DEFAULT_RULES).is_err());
    }

    // ── Custom rules ─────────────────────────────────────────

    #[test]
    fn custom_rules_applied() {
        // Arrange: custom rule requiring 90% for programs ≤ 60min
        let rules = [DurationCheckRule {
            min_min: 0,
            max_min: 60,
            min_percent: 90,
        }];
        let ts = 1800.0; // 30min
        let avs = 1620.0; // 90% (≤ 90% fails)

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, &rules).is_err());
    }

    #[test]
    fn custom_rules_passes() {
        // Arrange
        let rules = [DurationCheckRule {
            min_min: 0,
            max_min: 60,
            min_percent: 90,
        }];
        let ts = 1800.0;
        let avs = 1638.0; // 91%

        // Act / Assert
        assert!(validate_duration_ratio(ts, avs, &rules).is_ok());
    }

    // ── Empty rules ──────────────────────────────────────────

    #[test]
    fn empty_rules_always_passes() {
        // Arrange / Act / Assert
        assert!(validate_duration_ratio(3600.0, 100.0, &[]).is_ok());
    }

    // ── No matching rule (gap between 10 and 11 min) ─────────

    // ── check_pre_encode_duration via write_script ──────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_check_pre_encode_duration_passes() {
        // Arrange: fake ffprobe returning durations
        let dir = tempfile::tempdir().unwrap();
        // Script returns different values per invocation: first 1800 (TS), then 1440 (AVS = 80%)
        let script = crate::command::test_utils::write_script(
            dir.path(),
            "ffprobe.sh",
            "#!/bin/sh\nif [ -f /tmp/dtvmgr_test_second_call ]; then echo '1440.0'; rm /tmp/dtvmgr_test_second_call; else echo '1800.0'; touch /tmp/dtvmgr_test_second_call; fi",
        );
        let ts_file = dir.path().join("input.ts");
        let avs_file = dir.path().join("input.avs");
        std::fs::write(&ts_file, "dummy").unwrap();
        std::fs::write(&avs_file, "dummy").unwrap();

        // Act
        let result = check_pre_encode_duration(&script, &ts_file, &avs_file, None);

        // Assert — should pass (80% > 75% for ~30min)
        assert!(result.is_ok());
        // Cleanup
        let _ = std::fs::remove_file("/tmp/dtvmgr_test_second_call");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_check_pre_encode_duration_ffprobe_fails() {
        // Arrange: fake ffprobe that fails
        let dir = tempfile::tempdir().unwrap();
        let script =
            crate::command::test_utils::write_script(dir.path(), "ffprobe.sh", "#!/bin/sh\nexit 1");
        let ts_file = dir.path().join("input.ts");
        let avs_file = dir.path().join("input.avs");
        std::fs::write(&ts_file, "dummy").unwrap();
        std::fs::write(&avs_file, "dummy").unwrap();

        // Act
        let result = check_pre_encode_duration(&script, &ts_file, &avs_file, None);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn gap_between_rules_passes() {
        // Arrange: 10.5min = 630s, rounds to 11min so matches medium rule
        // Use a value that falls exactly in a gap if rules had one
        // With default rules, 10min and 11min are covered.
        // This tests the "no rule matched" path with custom rules.
        let rules = [DurationCheckRule {
            min_min: 0,
            max_min: 5,
            min_percent: 90,
        }];
        let ts = 600.0; // 10min — not covered by rule 0–5min
        let avs = 1.0; // very low ratio

        // Act / Assert: no rule matches, so check passes
        assert!(validate_duration_ratio(ts, avs, &rules).is_ok());
    }
}
