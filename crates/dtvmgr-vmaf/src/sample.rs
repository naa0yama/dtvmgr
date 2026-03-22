//! Sample extraction from TS files.
//!
//! Extracts short video segments from the input TS using `ffmpeg -c:v copy`
//! and generates lossless FFV1 reference files for VMAF comparison.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use tracing::{debug, info, instrument};

use crate::types::{ContentSegment, SampleConfig, SearchConfig, SearchProgress};

/// A sample file on disk together with its lossless reference.
#[derive(Debug)]
pub(crate) struct SampleFile {
    /// Path to the stream-copied sample (`.ts`).
    pub(crate) sample_path: PathBuf,
    /// Path to the lossless FFV1 reference (`.mkv`).
    pub(crate) reference_path: PathBuf,
    /// Size of the stream-copied sample in bytes.
    pub(crate) sample_size: u64,
}

/// Compute sample positions within the content segments.
///
/// Returns timestamps (in seconds) for each sample, distributed
/// evenly across the content after trimming `skip_secs` from the
/// start and end.
#[must_use]
pub(crate) fn compute_sample_positions(
    segments: &[ContentSegment],
    sample_cfg: &SampleConfig,
) -> Vec<f64> {
    // Compute total content duration
    let total_duration: f64 = segments.iter().map(ContentSegment::duration).sum();

    // Effective range after skipping start/end
    let effective_start = sample_cfg.skip_secs;
    let effective_end = (total_duration - sample_cfg.skip_secs).max(effective_start);
    let effective_duration = effective_end - effective_start;

    if effective_duration <= 0.0 {
        return Vec::new();
    }

    // Determine sample count.
    // Take one sample per `sample_every` seconds of content (ab-av1 style),
    // then clamp to [min_samples, max_samples].
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions
    )]
    let desired = (effective_duration / sample_cfg.sample_every_secs).ceil() as u32;
    let count = desired
        .clamp(sample_cfg.min_samples, sample_cfg.max_samples)
        .max(1);

    // Distribute samples evenly within the effective range.
    // Uses the same gap-based formula as ab-av1:
    //   gap = (effective_duration - sample_duration * count) / (count + 1)
    //   position[n] = effective_start + gap * (n+1) + sample_duration * n
    let count_f = f64::from(count);
    let gap = sample_cfg
        .duration_secs
        .mul_add(-count_f, effective_duration)
        / (count_f + 1.0);
    let gap = gap.max(0.0);

    let mut positions = Vec::with_capacity(count.try_into().unwrap_or(20));
    for i in 0..count {
        let n = f64::from(i);
        let content_offset = sample_cfg
            .duration_secs
            .mul_add(n, gap.mul_add(n + 1.0, effective_start));
        // Map content-relative offset to an absolute TS timestamp
        if let Some(abs_time) = content_offset_to_absolute(segments, content_offset) {
            positions.push(abs_time);
        }
    }

    positions
}

/// Map a content-relative offset (seconds from start of all content)
/// to an absolute timestamp in the TS file.
fn content_offset_to_absolute(segments: &[ContentSegment], offset: f64) -> Option<f64> {
    let mut remaining = offset;
    for seg in segments {
        let dur = seg.duration();
        if remaining <= dur {
            return Some(seg.start_secs + remaining);
        }
        remaining -= dur;
    }
    // Past the end of all segments — return end of last segment
    segments.last().map(|s| s.end_secs)
}

/// Extract samples from the TS file and create lossless references.
///
/// # Errors
///
/// Returns an error if ffmpeg fails to extract or encode a sample.
#[instrument(skip_all, err(level = "error"))]
pub(crate) fn extract_samples(
    config: &SearchConfig,
    on_progress: Option<&dyn Fn(SearchProgress)>,
) -> Result<Vec<SampleFile>> {
    let positions = compute_sample_positions(&config.content_segments, &config.sample);

    if positions.is_empty() {
        bail!("no valid sample positions found in content segments");
    }

    let temp_dir = config.temp_dir.clone().unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&temp_dir)
        .with_context(|| format!("failed to create temp dir: {}", temp_dir.display()))?;

    #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
    let total = positions.len() as u32;
    let mut samples = Vec::with_capacity(positions.len());

    for (i, &start_time) in positions.iter().enumerate() {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::as_conversions,
            clippy::arithmetic_side_effects
        )]
        let current = (i as u32) + 1;

        if let Some(cb) = on_progress {
            cb(SearchProgress::SampleExtract { current, total });
        }

        let sample_path = temp_dir.join(format!("vmaf_sample_{current:03}.ts"));
        let reference_path = temp_dir.join(format!("vmaf_ref_{current:03}.mkv"));

        // NOTEST(external-cmd): requires ffmpeg — sample extraction loop
        extract_copy(
            &config.ffmpeg_bin,
            &config.input_file,
            start_time,
            config.sample.duration_secs,
            &sample_path,
        )
        .with_context(|| format!("failed to extract sample {current}"))?;

        let sample_size = std::fs::metadata(&sample_path)
            .with_context(|| format!("failed to stat sample {current}"))?
            .len();

        create_reference(
            &config.ffmpeg_bin,
            &sample_path,
            config.effective_reference_filter(),
            &config.extra_input_args,
            &reference_path,
        )
        .with_context(|| format!("failed to create reference for sample {current}"))?;

        debug!(
            sample = current,
            total, start_time, sample_size, "sample extracted"
        );

        samples.push(SampleFile {
            sample_path,
            reference_path,
            sample_size,
        });
    }

    Ok(samples)
}

/// Stream-copy a segment from the TS file.
///
/// ```text
/// ffmpeg -y -ss {start} -t {duration} -i {input} -c:v copy -an {output}
/// ```
// NOTEST(external-cmd): requires ffmpeg — stream copy extraction
fn extract_copy(
    ffmpeg: &Path,
    input: &Path,
    start_secs: f64,
    duration_secs: f64,
    output: &Path,
) -> Result<()> {
    let start_str = format!("{start_secs:.6}");
    let dur_str = format!("{duration_secs:.6}");

    let args = [
        OsStr::new("-y"),
        OsStr::new("-ss"),
        OsStr::new(&start_str),
        OsStr::new("-t"),
        OsStr::new(&dur_str),
        OsStr::new("-i"),
        input.as_os_str(),
        OsStr::new("-c:v"),
        OsStr::new("copy"),
        OsStr::new("-an"),
        OsStr::new("-sn"),
        OsStr::new("-hide_banner"),
        OsStr::new("-loglevel"),
        OsStr::new("error"),
        output.as_os_str(),
    ];

    info!(cmd = %ffmpeg.display(), ?args, "running command (extract copy)");

    let status = Command::new(ffmpeg)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("failed to spawn ffmpeg for sample extraction")?;

    if !status.success() {
        bail!(
            "ffmpeg sample extraction exited with code {}",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}

/// Create a lossless FFV1 reference from a sample.
///
/// ```text
/// ffmpeg -y -i {sample} -vf {filter} -c:v ffv1 -an {output}
/// ```
// NOTEST(external-cmd): requires ffmpeg — FFV1 reference creation
fn create_reference(
    ffmpeg: &Path,
    sample: &Path,
    video_filter: &str,
    extra_input_args: &[String],
    output: &Path,
) -> Result<()> {
    let mut args: Vec<&OsStr> = Vec::with_capacity(20);
    args.push(OsStr::new("-y"));

    // HW device init args must appear before -i
    for arg in extra_input_args {
        args.push(OsStr::new(arg));
    }

    args.push(OsStr::new("-i"));
    args.push(sample.as_os_str());
    args.push(OsStr::new("-vf"));
    args.push(OsStr::new(video_filter));
    args.push(OsStr::new("-c:v"));
    args.push(OsStr::new("ffv1"));
    args.push(OsStr::new("-an"));
    args.push(OsStr::new("-hide_banner"));
    args.push(OsStr::new("-loglevel"));
    args.push(OsStr::new("error"));
    args.push(output.as_os_str());

    info!(cmd = %ffmpeg.display(), ?args, "running command (create reference)");

    let status = Command::new(ffmpeg)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("failed to spawn ffmpeg for reference creation")?;

    if !status.success() {
        bail!(
            "ffmpeg reference creation exited with code {}",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod test_utils {
    /// Creates a temporary executable shell script with the given body.
    ///
    /// Uses a subprocess (`sh -c "cat > file && chmod …"`) to write the
    /// script so that the writing fd is owned by a child process.  When
    /// `wait()` returns, the child has fully exited and the kernel has
    /// reaped all its fds, guaranteeing `i_writecount == 0` on the inode.
    /// This avoids `ETXTBSY` on overlayfs (Docker containers in CI).
    #[cfg(unix)]
    pub fn write_script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::io::Write;

        let target = dir.join(name);

        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(
                "cat > '{}' && chmod 755 '{}'",
                target.display(),
                target.display()
            ))
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        // Close stdin after writing to signal EOF to cat.
        {
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(body.as_bytes()).unwrap();
        }

        let status = child.wait().unwrap();
        assert!(status.success());

        target
    }

    /// Create a mock ffmpeg script that handles all command patterns.
    ///
    /// Detects the mode by inspecting arguments:
    /// - VMAF measurement (`-filter_complex` with `libvmaf`): writes VMAF
    ///   score to stderr.
    /// - All other modes: creates a small output file at the last argument
    ///   position (unless the last arg is `-`).
    #[cfg(unix)]
    pub fn write_mock_ffmpeg(dir: &std::path::Path) -> std::path::PathBuf {
        let body = r#"#!/bin/bash
# Check if this is a VMAF measurement (has -filter_complex with libvmaf)
for arg in "$@"; do
    if [[ "$arg" == *"libvmaf"* ]]; then
        echo "[Parsed_libvmaf_0 @ 0x0] VMAF score: 94.500000" >&2
        exit 0
    fi
done
# Otherwise, create the output file (last argument).
# Skip if last arg is "-" (null output).
output="${@: -1}"
if [[ "$output" != "-" ]]; then
    echo "mock" > "$output"
fi
exit 0
"#;
        write_script(dir, "mock_ffmpeg", body)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn segments(ranges: &[(f64, f64)]) -> Vec<ContentSegment> {
        ranges
            .iter()
            .map(|&(s, e)| ContentSegment {
                start_secs: s,
                end_secs: e,
            })
            .collect()
    }

    #[test]
    fn sample_positions_very_long_content_uses_sample_every() {
        // Arrange — 4 hours = 14400s, should use sample_every_secs
        let segs = segments(&[(0.0, 14400.0)]);
        let cfg = SampleConfig {
            duration_secs: 3.0,
            skip_secs: 120.0,
            sample_every_secs: 720.0,
            min_samples: 5,
            max_samples: 15,
            vmaf_subsample: 5,
        };

        // Act
        let positions = compute_sample_positions(&segs, &cfg);

        // Assert — effective = 14400 - 240 = 14160s, 14160/720 = 19.67 → ceil = 20,
        //          clamped to max_samples = 15
        assert_eq!(
            positions.len(),
            15,
            "expected max_samples clamped to 15, got {}",
            positions.len()
        );
    }

    #[test]
    fn sample_positions_skip_secs_zero_uses_full_duration() {
        // Arrange — skip_secs = 0 means full content is eligible
        let segs = segments(&[(0.0, 600.0)]);
        let cfg = SampleConfig {
            duration_secs: 3.0,
            skip_secs: 0.0,
            sample_every_secs: 720.0,
            min_samples: 5,
            max_samples: 15,
            vmaf_subsample: 5,
        };

        // Act
        let positions = compute_sample_positions(&segs, &cfg);

        // Assert — effective = 600s, 600/720 = ceil 1 → clamped to min=5
        assert_eq!(
            positions.len(),
            5,
            "expected min_samples 5, got {}",
            positions.len()
        );
        // First sample should start near 0 (no skip)
        let first = *positions.first().unwrap();
        assert!(
            first < 120.0,
            "first sample at {first} should be early (no skip)"
        );
    }

    #[test]
    fn sample_positions_single_short_segment_min_clamped() {
        // Arrange — 10s segment, skip=0, min_samples should clamp
        let segs = segments(&[(50.0, 60.0)]); // 10s segment
        let cfg = SampleConfig {
            duration_secs: 3.0,
            skip_secs: 0.0,
            sample_every_secs: 720.0,
            min_samples: 5,
            max_samples: 15,
            vmaf_subsample: 5,
        };

        // Act
        let positions = compute_sample_positions(&segs, &cfg);

        // Assert — even though content is only 10s, min_samples=5 is requested.
        // gap would be negative so clamped to 0, samples overlap but positions are generated.
        assert_eq!(
            positions.len(),
            5,
            "expected min_samples 5, got {}",
            positions.len()
        );
        // All positions within the segment
        for &pos in &positions {
            assert!(
                (50.0..=60.0).contains(&pos),
                "position {pos} out of segment bounds"
            );
        }
    }

    #[test]
    fn content_offset_to_absolute_empty_segments() {
        // Arrange
        let segs: Vec<ContentSegment> = Vec::new();

        // Act
        let result = content_offset_to_absolute(&segs, 10.0);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn content_offset_to_absolute_past_all_segments() {
        // Arrange — two segments with total 200s content
        let segs = segments(&[(100.0, 200.0), (300.0, 400.0)]);

        // Act — offset 500s is well past total content (200s)
        let result = content_offset_to_absolute(&segs, 500.0);

        // Assert — should return end of last segment
        assert_eq!(result, Some(400.0));
    }

    #[test]
    fn sample_positions_basic_24min() {
        // Arrange — 24 min single segment, skip 2 min each side
        let segs = segments(&[(0.0, 1440.0)]);
        let cfg = SampleConfig {
            duration_secs: 3.0,
            skip_secs: 120.0,
            min_samples: 10,
            max_samples: 20,
            ..SampleConfig::default()
        };

        // Act
        let positions = compute_sample_positions(&segs, &cfg);

        // Assert
        assert!(
            positions.len() >= 10,
            "expected >= 10 samples, got {}",
            positions.len()
        );
        assert!(
            positions.len() <= 20,
            "expected <= 20 samples, got {}",
            positions.len()
        );

        // All positions within bounds
        for &pos in &positions {
            assert!(pos >= 120.0, "sample at {pos}s is before skip zone (120s)");
            assert!(
                pos + 3.0 <= 1320.0,
                "sample at {pos}s extends past skip zone (1320s)"
            );
        }
    }

    #[test]
    fn sample_positions_short_content() {
        // Arrange — 5 min content, skip would eat everything
        let segs = segments(&[(10.0, 310.0)]); // 300s = 5 min
        let cfg = SampleConfig {
            duration_secs: 3.0,
            skip_secs: 180.0, // 3 min skip > half of content
            min_samples: 5,
            max_samples: 10,
            ..SampleConfig::default()
        };

        // Act
        let positions = compute_sample_positions(&segs, &cfg);

        // Assert — should have no positions since effective duration is 0
        assert!(positions.is_empty());
    }

    #[test]
    fn sample_positions_multiple_segments() {
        // Arrange — two segments (simulating split content)
        let segs = segments(&[(100.0, 400.0), (500.0, 800.0)]);
        // Total content: 300 + 300 = 600s
        let cfg = SampleConfig {
            duration_secs: 3.0,
            skip_secs: 30.0,
            min_samples: 5,
            max_samples: 15,
            ..SampleConfig::default()
        };

        // Act
        let positions = compute_sample_positions(&segs, &cfg);

        // Assert
        assert!(!positions.is_empty());
        // All positions should be within actual segment ranges
        for &pos in &positions {
            let in_any_segment = segs.iter().any(|s| pos >= s.start_secs && pos < s.end_secs);
            assert!(in_any_segment, "position {pos}s not in any segment");
        }
    }

    #[test]
    fn content_offset_to_absolute_single_segment() {
        let segs = segments(&[(100.0, 400.0)]);
        assert_eq!(content_offset_to_absolute(&segs, 0.0), Some(100.0));
        assert_eq!(content_offset_to_absolute(&segs, 150.0), Some(250.0));
        assert_eq!(content_offset_to_absolute(&segs, 300.0), Some(400.0));
    }

    #[test]
    fn content_offset_to_absolute_multi_segment() {
        let segs = segments(&[(100.0, 200.0), (300.0, 500.0)]);
        // First segment covers content offset 0..100 (100s duration)
        assert_eq!(content_offset_to_absolute(&segs, 50.0), Some(150.0));
        // offset 100 still within first segment (end boundary)
        assert_eq!(content_offset_to_absolute(&segs, 100.0), Some(200.0));
        // offset 101 enters second segment → 300.0 + 1.0
        assert_eq!(content_offset_to_absolute(&segs, 101.0), Some(301.0));
        assert_eq!(content_offset_to_absolute(&segs, 150.0), Some(350.0));
    }

    #[test]
    fn sample_positions_skip_secs_equals_half_duration() {
        // Arrange — skip_secs exactly half of total duration (tight effective range)
        let segs = segments(&[(0.0, 400.0)]); // 400s
        let cfg = SampleConfig {
            duration_secs: 3.0,
            skip_secs: 200.0, // effective = 400 - 2*200 = 0
            sample_every_secs: 720.0,
            min_samples: 5,
            max_samples: 15,
            vmaf_subsample: 5,
        };

        // Act
        let positions = compute_sample_positions(&segs, &cfg);

        // Assert — effective duration is 0, so no positions
        assert!(positions.is_empty());
    }

    #[test]
    fn sample_positions_exact_min_samples() {
        // Arrange — content duration that yields exactly min_samples
        let segs = segments(&[(0.0, 3600.0)]); // 1 hour
        let cfg = SampleConfig {
            duration_secs: 3.0,
            skip_secs: 0.0,
            sample_every_secs: 600.0, // 3600/600 = 6 → between min and max
            min_samples: 5,
            max_samples: 15,
            vmaf_subsample: 5,
        };

        // Act
        let positions = compute_sample_positions(&segs, &cfg);

        // Assert
        assert_eq!(positions.len(), 6);
        // Positions should be monotonically increasing
        for i in 1..positions.len() {
            assert!(
                positions[i] > positions[i - 1],
                "positions should be monotonic: {} <= {}",
                positions[i],
                positions[i - 1]
            );
        }
    }

    #[test]
    fn content_offset_to_absolute_exact_boundary() {
        // Arrange — offset exactly at segment boundary
        let segs = segments(&[(0.0, 100.0), (200.0, 300.0)]);

        // Act — offset exactly at boundary of first segment
        let result = content_offset_to_absolute(&segs, 100.0);

        // Assert — 100s of first segment maps to its end (100.0)
        assert_eq!(result, Some(100.0));
    }

    #[test]
    fn sample_positions_single_sample() {
        // Arrange — very short content with min_samples = 1
        let segs = segments(&[(0.0, 10.0)]);
        let cfg = SampleConfig {
            duration_secs: 3.0,
            skip_secs: 0.0,
            sample_every_secs: 720.0,
            min_samples: 1,
            max_samples: 1,
            vmaf_subsample: 5,
        };

        // Act
        let positions = compute_sample_positions(&segs, &cfg);

        // Assert
        assert_eq!(positions.len(), 1);
        let first = positions.first().unwrap();
        assert!((0.0..=10.0).contains(first));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn extract_samples_with_mock_ffmpeg() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let mock_ffmpeg = test_utils::write_mock_ffmpeg(dir.path());

        // Create a fake TS input (mock ffmpeg doesn't read it)
        let input_ts = dir.path().join("input.ts");
        std::fs::write(&input_ts, b"fake ts data").unwrap();

        let config = SearchConfig {
            ffmpeg_bin: mock_ffmpeg,
            input_file: input_ts,
            content_segments: vec![ContentSegment {
                start_secs: 0.0,
                end_secs: 600.0,
            }],
            encoder: crate::EncoderConfig::libx264(),
            video_filter: String::from("null"),
            target_vmaf: 93.0,
            max_encoded_percent: 80.0,
            min_vmaf_tolerance: 1.0,
            thorough: false,
            sample: SampleConfig {
                duration_secs: 3.0,
                skip_secs: 0.0,
                sample_every_secs: 720.0,
                min_samples: 2,
                max_samples: 3,
                vmaf_subsample: 5,
            },
            extra_encode_args: Vec::new(),
            extra_input_args: Vec::new(),
            reference_filter: None,
            temp_dir: Some(dir.path().to_owned()),
        };

        // Act
        let samples = extract_samples(&config, None).unwrap();

        // Assert — should have extracted 2 samples (min_samples)
        assert_eq!(
            samples.len(),
            2,
            "expected 2 samples, got {}",
            samples.len()
        );
        for sample in &samples {
            assert!(
                sample.sample_path.exists(),
                "sample path should exist: {}",
                sample.sample_path.display()
            );
            assert!(
                sample.reference_path.exists(),
                "reference path should exist: {}",
                sample.reference_path.display()
            );
            assert!(sample.sample_size > 0, "sample size should be non-zero");
        }
    }
}
