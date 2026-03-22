//! VMAF measurement via ffmpeg's libvmaf filter.
//!
//! Both the distorted and reference inputs are upscaled to 1920×1080
//! before measurement so that the standard VMAF model (calibrated for
//! 1080p viewing) produces meaningful scores.

use std::ffi::OsStr;
use std::io::{BufRead as _, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use tracing::{info, instrument};

/// The string prefix ffmpeg prints before the final VMAF score.
const SCORE_PREFIX: &str = "VMAF score: ";

/// Measure the VMAF score between an encoded (distorted) file and a
/// lossless reference.
///
/// Both inputs are upscaled to 1920×1080 via the `filter_complex` so
/// that the standard `vmaf_v0.6.1` model (trained on 1080p content)
/// produces calibrated scores.
///
/// # Errors
///
/// Returns an error if ffmpeg cannot be spawned, exits with a non-zero
/// status, or does not produce a parseable VMAF score line.
#[instrument(skip_all, err(level = "error"))]
// NOTEST(external-cmd): requires ffmpeg — VMAF measurement
pub(crate) fn measure_vmaf(
    ffmpeg: &Path,
    distorted: &Path,
    reference: &Path,
    n_subsample: u32,
) -> Result<f32> {
    let filter_complex = format!(
        "[0:v]scale=1920:1080:flags=bicubic[distorted];\
         [1:v]scale=1920:1080:flags=bicubic[reference];\
         [distorted][reference]libvmaf=model=version=vmaf_v0.6.1:n_subsample={n_subsample}"
    );

    let args = [
        OsStr::new("-i"),
        distorted.as_os_str(),
        OsStr::new("-i"),
        reference.as_os_str(),
        OsStr::new("-filter_complex"),
        OsStr::new(&filter_complex),
        OsStr::new("-an"),
        OsStr::new("-sn"),
        OsStr::new("-dn"),
        OsStr::new("-hide_banner"),
        OsStr::new("-f"),
        OsStr::new("null"),
        OsStr::new("-"),
    ];

    info!(cmd = %ffmpeg.display(), ?args, "running command (measure VMAF)");

    let mut child = Command::new(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn ffmpeg for VMAF measurement")?;

    // Read stderr line-by-line looking for the VMAF score.
    let stderr = child
        .stderr
        .take()
        .context("failed to capture ffmpeg stderr")?;
    let reader = BufReader::new(stderr);

    let mut vmaf_score: Option<f32> = None;

    for line in reader.lines() {
        let line = line.context("failed to read ffmpeg stderr line")?;

        if let Some(idx) = line.find(SCORE_PREFIX) {
            #[allow(clippy::arithmetic_side_effects)]
            let score_str = line[idx + SCORE_PREFIX.len()..].trim();
            if let Ok(score) = score_str.parse::<f32>() {
                vmaf_score = Some(score);
            }
        }
    }

    let status = child
        .wait()
        .context("failed to wait for ffmpeg VMAF process")?;

    if !status.success() {
        bail!(
            "ffmpeg VMAF measurement exited with code {}",
            status.code().unwrap_or(-1)
        );
    }

    vmaf_score.context("failed to parse VMAF score from ffmpeg output")
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn score_prefix_detection() {
        // Arrange
        let line = "[Parsed_libvmaf_6 @ 000002b296bac480] VMAF score: 94.826380";

        // Act
        let idx = line.find(SCORE_PREFIX).unwrap();
        #[allow(clippy::arithmetic_side_effects)]
        let score_str = line[idx + SCORE_PREFIX.len()..].trim();
        let score: f32 = score_str.parse().unwrap();

        // Assert
        assert!((score - 94.826_38).abs() < 0.001);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn measure_vmaf_with_mock_ffmpeg() {
        use crate::sample::test_utils::write_mock_ffmpeg;

        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let mock_ffmpeg = write_mock_ffmpeg(dir.path());

        let distorted = dir.path().join("distorted.mkv");
        let reference = dir.path().join("reference.mkv");
        std::fs::write(&distorted, b"fake distorted").unwrap();
        std::fs::write(&reference, b"fake reference").unwrap();

        // Act
        let score = measure_vmaf(&mock_ffmpeg, &distorted, &reference, 5).unwrap();

        // Assert — mock outputs 94.5
        assert!(
            (score - 94.5).abs() < 0.01,
            "expected VMAF score ~94.5, got {score}"
        );
    }
}
