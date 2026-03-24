//! Interpolated binary search for optimal quality parameter.
//!
//! Finds the highest quality-parameter value (= smallest file) that
//! still achieves the target VMAF score.  Uses VMAF-space linear
//! interpolation to converge quickly, with a tolerance-based
//! fallback when no exact match exists.

use anyhow::{Context, Result, bail};
use tracing::{debug, info, instrument, warn};

use std::path::Path;

use crate::encode;
use crate::encoder::{QualityConverter, SearchSample, vmaf_lerp_q};
use crate::sample::{self, SampleFile};
use crate::types::{SearchConfig, SearchProgress, SearchResult};
use crate::vmaf;

/// Maximum number of search iterations before giving up.
const MAX_ITERATIONS: u32 = 12;

/// Run the full quality search pipeline.
///
/// 1. Extract samples from TS (via `-c:v copy`).
/// 2. Create lossless references.
/// 3. Run interpolated binary search over quality values.
/// 4. Return the optimal quality value.
///
/// # Errors
///
/// Returns an error if the search cannot converge, or if any ffmpeg
/// operation fails.
#[allow(clippy::too_many_lines)]
#[instrument(skip_all, err(level = "error"))]
// NOTEST(external-cmd): requires ffmpeg — full search pipeline
pub(crate) fn run(
    config: &SearchConfig,
    on_progress: Option<&dyn Fn(SearchProgress)>,
) -> Result<SearchResult> {
    // Step 1–2: Extract samples and create references
    let samples =
        sample::extract_samples(config, on_progress).context("sample extraction failed")?;

    if samples.is_empty() {
        bail!("no samples could be extracted");
    }

    let temp_dir = config.temp_dir.clone().unwrap_or_else(std::env::temp_dir);

    // Step 3: Run binary search
    let conv = QualityConverter::new(&config.encoder);
    let (min_q, max_q) = conv.min_max_q(config.encoder.min_quality, config.encoder.max_quality);

    // Start at the hint (benchmark-derived sweet spot) instead of midpoint
    let initial_q = conv.q(config.encoder.quality_hint).clamp(min_q, max_q);

    let mut q = initial_q;
    let mut attempts: Vec<SearchSample> = Vec::new();

    info!(
        target_vmaf = config.target_vmaf,
        max_size_pct = config.max_encoded_percent,
        min_vmaf_tolerance = config.min_vmaf_tolerance,
        codec = %config.encoder.codec,
        hint = config.encoder.quality_hint,
        min_q,
        max_q,
        "starting quality search"
    );

    for run in 1..=MAX_ITERATIONS {
        // Skip if this q was already tested
        if attempts.iter().any(|a| a.q == q) {
            debug!(q, "q already tested, adjusting");
            // Try q+1 or q-1 that hasn't been tested
            #[allow(clippy::arithmetic_side_effects)]
            let candidates = [q + 1, q - 1];
            match candidates
                .iter()
                .find(|&&c| c >= min_q && c <= max_q && !attempts.iter().any(|a| a.q == c))
            {
                Some(&next) => q = next,
                None => break, // all adjacent tested
            }
        }

        let quality = conv.quality(q);

        // Encode all samples at this quality and measure VMAF
        let (mean_vmaf, size_percent) =
            evaluate_quality(config, &samples, quality, run, &temp_dir, on_progress)
                .with_context(|| format!("evaluation failed at quality {quality}"))?;

        let sample = SearchSample {
            q,
            quality,
            vmaf: mean_vmaf,
            size_percent,
        };

        if let Some(cb) = on_progress {
            cb(SearchProgress::IterationResult {
                iteration: run,
                quality,
                vmaf: mean_vmaf,
                size_percent,
            });
        }

        info!(
            iteration = run,
            quality = format!("{quality:.3}"),
            vmaf = format!("{mean_vmaf:.3}"),
            size_pct = format!("{size_percent:.1}"),
            "iteration result"
        );

        attempts.push(sample.clone());

        let small_enough = size_percent <= f64::from(config.max_encoded_percent);

        if mean_vmaf >= config.target_vmaf {
            // Quality meets target — try to push q higher (smaller file)
            // Check if adjacent q+1 is already tested and below target
            #[allow(clippy::arithmetic_side_effects)]
            let next_q = q + 1;
            let adjacent_below = attempts.iter().find(|a| a.q == next_q);

            match adjacent_below {
                Some(adj) if adj.vmaf < config.target_vmaf => {
                    // q is the best: meets target, q+1 does not
                    if small_enough {
                        info!(
                            quality = format!("{quality:.3}"),
                            vmaf = format!("{mean_vmaf:.3}"),
                            "search converged"
                        );
                        cleanup_samples(&samples);
                        return Ok(build_result(config, &sample, run));
                    }
                    // Meets VMAF but too large — no solution
                    bail!(
                        "quality {quality:.3} achieves VMAF {mean_vmaf:.3} but exceeds size limit {:.0}%",
                        config.max_encoded_percent
                    );
                }
                Some(_) => {
                    // q+1 also meets target — move higher
                    #[allow(clippy::arithmetic_side_effects)]
                    {
                        q = next_q + 1;
                    }
                    q = q.min(max_q);
                }
                None if next_q <= max_q => {
                    // q+1 not tested yet — test it next (greedy: try smaller file)
                    q = next_q;
                }
                None => {
                    // At max_q boundary — this is optimal
                    if small_enough {
                        info!(
                            quality = format!("{quality:.3}"),
                            vmaf = format!("{mean_vmaf:.3}"),
                            "converged at max"
                        );
                        cleanup_samples(&samples);
                        return Ok(build_result(config, &sample, run));
                    }
                    bail!(
                        "quality {quality:.3} achieves VMAF {mean_vmaf:.3} but exceeds size limit {:.0}%",
                        config.max_encoded_percent
                    );
                }
            }
        } else {
            // Quality below target — need higher quality (lower q)
            // Use interpolation if we have a point above target
            let above = attempts
                .iter()
                .filter(|a| a.vmaf >= config.target_vmaf)
                .min_by_key(|a| a.q);

            match above {
                #[allow(clippy::arithmetic_side_effects)]
                Some(better) if sample.q - better.q <= 1 => {
                    // Adjacent — better is the optimal value (meets target, sample does not)
                    if better.size_percent <= f64::from(config.max_encoded_percent) {
                        info!(
                            quality = format!("{:.3}", better.quality),
                            vmaf = format!("{:.3}", better.vmaf),
                            "converged at adjacent boundary"
                        );
                        cleanup_samples(&samples);
                        return Ok(build_result(config, better, run));
                    }
                    break; // Can't meet size constraint
                }
                Some(better) => {
                    // Interpolate between current (below) and better (above)
                    q = vmaf_lerp_q(config.target_vmaf, &sample, better);
                }
                None if q > min_q => {
                    // No point above target yet — move toward min_q
                    #[allow(clippy::arithmetic_side_effects)]
                    let step = ((q - min_q) / 2).max(1);
                    #[allow(clippy::arithmetic_side_effects)]
                    {
                        q -= step;
                    }
                }
                None => {
                    // Already at min_q — cannot improve further
                    break;
                }
            }
        }

        debug!(next_q = q, next_quality = conv.quality(q), "next iteration");
    }

    // Clean up sample and reference files
    cleanup_samples(&samples);

    // Search exhausted — select the best result
    select_best_result(config, &attempts)
}

/// Select the best result from all attempts.
///
/// Priority:
/// 1. Highest q (smallest file) that meets target VMAF and size limit
/// 2. Highest q that meets (target - tolerance) and size limit
/// 3. Error if nothing qualifies
fn select_best_result(config: &SearchConfig, attempts: &[SearchSample]) -> Result<SearchResult> {
    // Priority 1: meets target exactly
    if let Some(best) = attempts
        .iter()
        .filter(|s| s.vmaf >= config.target_vmaf)
        .filter(|s| s.size_percent <= f64::from(config.max_encoded_percent))
        .max_by_key(|s| s.q)
    {
        info!(
            quality = format!("{:.3}", best.quality),
            vmaf = format!("{:.3}", best.vmaf),
            "selected best result (meets target)"
        );
        #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
        return Ok(build_result(config, best, attempts.len() as u32));
    }

    // Priority 2: meets target - tolerance (fallback)
    let min_acceptable = config.target_vmaf - config.min_vmaf_tolerance;
    if let Some(best) = attempts
        .iter()
        .filter(|s| s.vmaf >= min_acceptable)
        .filter(|s| s.size_percent <= f64::from(config.max_encoded_percent))
        .max_by_key(|s| s.q)
    {
        warn!(
            quality = format!("{:.3}", best.quality),
            vmaf = format!("{:.3}", best.vmaf),
            target = format!("{:.3}", config.target_vmaf),
            tolerance = format!("{:.3}", config.min_vmaf_tolerance),
            "selected fallback result (within tolerance)"
        );
        #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
        return Ok(build_result(config, best, attempts.len() as u32));
    }

    // Nothing qualifies
    let best_vmaf = attempts.iter().max_by(|a, b| {
        a.vmaf
            .partial_cmp(&b.vmaf)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    match best_vmaf {
        Some(b) => bail!(
            "no quality value achieves VMAF {:.3} (best: {:.3} at quality {:.3}, tolerance {:.3})",
            config.target_vmaf,
            b.vmaf,
            b.quality,
            config.min_vmaf_tolerance
        ),
        None => bail!("no search attempts recorded"),
    }
}

/// Encode all samples at the given quality and measure mean VMAF.
///
/// Returns `(mean_vmaf, mean_size_percent)`.
// NOTEST(external-cmd): requires ffmpeg — sample encode + VMAF
fn evaluate_quality(
    config: &SearchConfig,
    samples: &[SampleFile],
    quality: f32,
    iteration: u32,
    temp_dir: &Path,
    on_progress: Option<&dyn Fn(SearchProgress)>,
) -> Result<(f32, f64)> {
    let mut total_vmaf = 0.0_f32;
    let mut total_sample_size = 0_u64;
    let mut total_encoded_size = 0_u64;

    #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
    let total = samples.len() as u32;

    for (i, sample) in samples.iter().enumerate() {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            clippy::arithmetic_side_effects
        )]
        let current = (i as u32) + 1;

        if let Some(cb) = on_progress {
            cb(SearchProgress::Encoding {
                iteration,
                quality,
                sample: current,
                total,
            });
        }

        // Encode at candidate quality
        let encoded_path = temp_dir.join(format!("vmaf_enc_{iteration:02}_{current:03}.mkv"));
        let enc_result = encode::encode_sample(
            &config.ffmpeg_bin,
            &sample.sample_path,
            &config.video_filter,
            &config.encoder,
            quality,
            &config.extra_encode_args,
            &config.extra_input_args,
            &encoded_path,
        )?;

        if let Some(cb) = on_progress {
            cb(SearchProgress::Scoring {
                iteration,
                quality,
                sample: current,
                total,
            });
        }

        // Measure VMAF (both upscaled to 1080p)
        let vmaf_score = vmaf::measure_vmaf(
            &config.ffmpeg_bin,
            &enc_result.path,
            &sample.reference_path,
            config.sample.vmaf_subsample,
        )?;

        debug!(
            iteration,
            sample = current,
            quality = format!("{quality:.3}"),
            vmaf = format!("{vmaf_score:.3}"),
            encoded_bytes = enc_result.encoded_size,
            "sample score"
        );

        total_vmaf += vmaf_score;
        #[allow(clippy::arithmetic_side_effects)]
        {
            total_sample_size += sample.sample_size;
            total_encoded_size += enc_result.encoded_size;
        }

        // Clean up encoded sample
        let _ = std::fs::remove_file(&encoded_path);
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        clippy::arithmetic_side_effects
    )]
    let mean_vmaf = total_vmaf / samples.len() as f32;
    #[allow(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        clippy::arithmetic_side_effects
    )]
    let size_percent = if total_sample_size > 0 {
        total_encoded_size as f64 * 100.0 / total_sample_size as f64
    } else {
        0.0
    };

    Ok((mean_vmaf, size_percent))
}

/// Remove sample and reference files from disk.
fn cleanup_samples(samples: &[SampleFile]) {
    for sample in samples {
        let _ = std::fs::remove_file(&sample.sample_path);
        let _ = std::fs::remove_file(&sample.reference_path);
    }
}

/// Build the final [`SearchResult`] from a converged sample.
fn build_result(config: &SearchConfig, sample: &SearchSample, iterations: u32) -> SearchResult {
    SearchResult {
        quality_value: sample.quality,
        quality_param: String::from(config.encoder.quality_param.flag()),
        mean_vmaf: sample.vmaf,
        predicted_size_percent: sample.size_percent,
        iterations,
    }
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::PathBuf;

    use super::*;
    use crate::types::{ContentSegment, EncoderConfig, SampleConfig, SearchConfig};

    /// Build a minimal `SearchConfig` for unit testing (no I/O).
    fn test_config() -> SearchConfig {
        SearchConfig {
            ffmpeg_bin: PathBuf::from("ffmpeg"),
            input_file: PathBuf::from("input.ts"),
            content_segments: vec![ContentSegment {
                start_secs: 0.0,
                end_secs: 1440.0,
            }],
            encoder: EncoderConfig::libx264(),
            video_filter: String::from("null"),
            target_vmaf: 93.0,
            max_encoded_percent: 80.0,
            min_vmaf_tolerance: 1.0,
            thorough: false,
            sample: SampleConfig::default(),
            extra_encode_args: Vec::new(),
            extra_input_args: Vec::new(),
            reference_filter: None,
            temp_dir: None,
        }
    }

    // ── cleanup_samples ─────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn cleanup_samples_no_panic_on_nonexistent_files() {
        // Arrange — paths that do not exist on disk
        let samples = vec![
            SampleFile {
                sample_path: PathBuf::from("/tmp/nonexistent_vmaf_sample_001.ts"),
                reference_path: PathBuf::from("/tmp/nonexistent_vmaf_ref_001.mkv"),
                sample_size: 0,
            },
            SampleFile {
                sample_path: PathBuf::from("/tmp/nonexistent_vmaf_sample_002.ts"),
                reference_path: PathBuf::from("/tmp/nonexistent_vmaf_ref_002.mkv"),
                sample_size: 0,
            },
        ];

        // Act / Assert — should not panic
        cleanup_samples(&samples);
    }

    // ── build_result ────────────────────────────────────────

    #[test]
    fn build_result_populates_all_fields() {
        // Arrange
        let config = test_config();
        let sample = SearchSample {
            q: 25,
            quality: 25.0,
            vmaf: 94.5,
            size_percent: 65.0,
        };

        // Act
        let result = build_result(&config, &sample, 7);

        // Assert
        assert!((result.quality_value - 25.0).abs() < f32::EPSILON);
        assert_eq!(result.quality_param, "-crf");
        assert!((result.mean_vmaf - 94.5).abs() < f32::EPSILON);
        assert!((result.predicted_size_percent - 65.0).abs() < f64::EPSILON);
        assert_eq!(result.iterations, 7);
    }

    // ── select_best_result ──────────────────────────────────

    #[test]
    fn select_best_result_priority1_meets_target() {
        // Arrange — one attempt meets target VMAF and size
        let config = test_config();
        let attempts = vec![
            SearchSample {
                q: 25,
                quality: 25.0,
                vmaf: 94.0,
                size_percent: 70.0,
            },
            SearchSample {
                q: 26,
                quality: 26.0,
                vmaf: 93.5,
                size_percent: 60.0,
            },
        ];

        // Act
        let result = select_best_result(&config, &attempts).unwrap();

        // Assert — should pick q=26 (highest q that meets target)
        assert!((result.quality_value - 26.0).abs() < f32::EPSILON);
        assert!((result.mean_vmaf - 93.5).abs() < f32::EPSILON);
    }

    #[test]
    fn select_best_result_priority2_within_tolerance() {
        // Arrange — no attempt meets target (93.0), but one is within tolerance (1.0)
        let config = test_config();
        let attempts = vec![
            SearchSample {
                q: 24,
                quality: 24.0,
                vmaf: 92.5, // within tolerance: 93.0 - 1.0 = 92.0
                size_percent: 75.0,
            },
            SearchSample {
                q: 25,
                quality: 25.0,
                vmaf: 92.0, // also within tolerance
                size_percent: 65.0,
            },
        ];

        // Act
        let result = select_best_result(&config, &attempts).unwrap();

        // Assert — should pick q=25 (highest q within tolerance)
        assert!((result.quality_value - 25.0).abs() < f32::EPSILON);
        assert!((result.mean_vmaf - 92.0).abs() < f32::EPSILON);
    }

    #[test]
    fn select_best_result_error_when_nothing_qualifies() {
        // Arrange — all attempts are below tolerance
        let config = test_config();
        let attempts = vec![SearchSample {
            q: 20,
            quality: 20.0,
            vmaf: 85.0,         // below 93.0 - 1.0 = 92.0
            size_percent: 90.0, // also exceeds max_encoded_percent
        }];

        // Act
        let result = select_best_result(&config, &attempts);

        // Assert
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("no quality value achieves VMAF"),
            "unexpected error: {err_msg}"
        );
    }

    #[test]
    fn select_best_result_error_when_no_attempts() {
        // Arrange — empty attempts vec
        let config = test_config();
        let attempts: Vec<SearchSample> = Vec::new();

        // Act
        let result = select_best_result(&config, &attempts);

        // Assert
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("no search attempts recorded"),
            "unexpected error: {err_msg}"
        );
    }

    #[test]
    fn select_best_result_all_below_tolerance_error() {
        // Arrange — all VMAF values below (target - tolerance) = 92.0
        let config = test_config(); // target=93, tolerance=1
        let attempts = vec![
            SearchSample {
                q: 20,
                quality: 20.0,
                vmaf: 91.5, // below 92.0
                size_percent: 50.0,
            },
            SearchSample {
                q: 22,
                quality: 22.0,
                vmaf: 89.0, // below 92.0
                size_percent: 40.0,
            },
        ];

        // Act
        let result = select_best_result(&config, &attempts);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn select_best_result_multiple_meet_target_picks_highest_q() {
        // Arrange — multiple attempts meet target, verify highest q selected
        let config = test_config(); // target=93, max_encoded=80
        let attempts = vec![
            SearchSample {
                q: 23,
                quality: 23.0,
                vmaf: 95.0,
                size_percent: 70.0,
            },
            SearchSample {
                q: 25,
                quality: 25.0,
                vmaf: 93.5,
                size_percent: 60.0,
            },
            SearchSample {
                q: 27,
                quality: 27.0,
                vmaf: 93.1,
                size_percent: 50.0,
            },
        ];

        // Act
        let result = select_best_result(&config, &attempts).unwrap();

        // Assert — q=27 is highest q that meets target (93.1 >= 93.0)
        assert!((result.quality_value - 27.0).abs() < f32::EPSILON);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn run_search_with_mock_ffmpeg() {
        use crate::sample::test_utils::write_mock_ffmpeg;

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
        let result = run(&config, None).unwrap();

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
    }

    #[test]
    fn select_best_result_good_vmaf_but_bad_size_skipped() {
        // Arrange — one attempt meets VMAF but exceeds size limit
        let config = test_config(); // max_encoded_percent=80
        let attempts = vec![
            SearchSample {
                q: 25,
                quality: 25.0,
                vmaf: 95.0,
                size_percent: 90.0, // exceeds 80% limit
            },
            SearchSample {
                q: 23,
                quality: 23.0,
                vmaf: 92.5, // within tolerance (>= 92.0)
                size_percent: 70.0,
            },
        ];

        // Act
        let result = select_best_result(&config, &attempts).unwrap();

        // Assert — q=25 skipped due to size, falls back to q=23 (tolerance)
        assert!((result.quality_value - 23.0).abs() < f32::EPSILON);
    }

    // ── cleanup_samples with real files ────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn cleanup_samples_deletes_actual_temp_files() {
        // Arrange — create actual temp files
        let dir = std::env::temp_dir().join("dtvmgr_vmaf_test_cleanup");
        std::fs::create_dir_all(&dir).unwrap();

        let sample_path = dir.join("test_sample_001.ts");
        let reference_path = dir.join("test_ref_001.mkv");
        std::fs::write(&sample_path, b"sample data").unwrap();
        std::fs::write(&reference_path, b"reference data").unwrap();

        assert!(sample_path.exists());
        assert!(reference_path.exists());

        let samples = vec![SampleFile {
            sample_path: sample_path.clone(),
            reference_path: reference_path.clone(),
            sample_size: 11,
        }];

        // Act
        cleanup_samples(&samples);

        // Assert — files should be deleted
        assert!(
            !sample_path.exists(),
            "sample file should have been deleted"
        );
        assert!(
            !reference_path.exists(),
            "reference file should have been deleted"
        );

        // Clean up test dir
        let _ = std::fs::remove_dir(&dir);
    }
}
