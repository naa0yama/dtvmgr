//! Sample encoding at a candidate quality value.
//!
//! Encodes a sample through the video filter chain (deinterlace + scale)
//! using the target encoder and quality parameter.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use tracing::{info, instrument};

use crate::types::EncoderConfig;

/// Result of encoding a single sample at a specific quality value.
#[derive(Debug, Clone)]
pub(crate) struct EncodeResult {
    /// Path to the encoded file.
    pub(crate) path: PathBuf,
    /// Size of the encoded file in bytes.
    pub(crate) encoded_size: u64,
}

/// Encode a sample at the given quality value.
///
/// Applies the video filter, codec, and quality parameter, producing
/// an encoded MKV file.
///
/// # Errors
///
/// Returns an error if ffmpeg fails.
#[allow(clippy::too_many_arguments)]
#[instrument(skip_all, fields(quality, codec = %encoder.codec), err(level = "error"))]
// NOTEST(external-cmd): requires ffmpeg — sample encoding
pub(crate) fn encode_sample(
    ffmpeg: &Path,
    sample: &Path,
    video_filter: &str,
    encoder: &EncoderConfig,
    quality: f32,
    extra_encode_args: &[String],
    extra_input_args: &[String],
    output: &Path,
) -> Result<EncodeResult> {
    let quality_str = format!("{quality}");
    let quality_flag_with_spec = format!("{}:v", encoder.quality_param.flag());

    let mut args: Vec<&OsStr> = Vec::with_capacity(32);

    args.push(OsStr::new("-y"));
    args.push(OsStr::new("-hide_banner"));
    args.push(OsStr::new("-loglevel"));
    args.push(OsStr::new("error"));

    for arg in extra_input_args {
        args.push(OsStr::new(arg));
    }

    for arg in &encoder.default_input_args {
        args.push(OsStr::new(arg));
    }

    args.push(OsStr::new("-i"));
    args.push(sample.as_os_str());

    args.push(OsStr::new("-c:v"));
    args.push(OsStr::new(&encoder.codec));

    args.push(OsStr::new(&quality_flag_with_spec));
    args.push(OsStr::new(&quality_str));

    args.push(OsStr::new("-vf"));
    args.push(OsStr::new(video_filter));

    let preset_val;
    if let Some(ref preset) = encoder.preset {
        args.push(OsStr::new("-preset:v"));
        preset_val = preset.clone();
        args.push(OsStr::new(&preset_val));
    }

    let pix_fmt_val;
    if let Some(ref pix_fmt) = encoder.pix_fmt {
        args.push(OsStr::new("-pix_fmt:v"));
        pix_fmt_val = pix_fmt.clone();
        args.push(OsStr::new(&pix_fmt_val));
    }

    // BT.709 color metadata for HD broadcast sources.
    let color_args: Vec<String> = crate::types::BT709_COLOR_ARGS
        .iter()
        .flat_map(|&(k, v)| [format!("{k}:v"), v.to_owned()])
        .collect();
    for arg in &color_args {
        args.push(OsStr::new(arg));
    }

    for arg in &encoder.default_ffmpeg_args {
        args.push(OsStr::new(arg));
    }

    for arg in extra_encode_args {
        args.push(OsStr::new(arg));
    }

    args.push(OsStr::new("-an"));

    args.push(output.as_os_str());

    info!(cmd = %ffmpeg.display(), ?args, "running command (encode sample)");

    let status = Command::new(ffmpeg)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("failed to spawn ffmpeg for sample encoding")?;

    if !status.success() {
        bail!(
            "ffmpeg sample encoding exited with code {}",
            status.code().unwrap_or(-1)
        );
    }

    let encoded_size = std::fs::metadata(output)
        .with_context(|| format!("failed to stat encoded sample: {}", output.display()))?
        .len();

    Ok(EncodeResult {
        path: output.to_owned(),
        encoded_size,
    })
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::sample::test_utils::write_mock_ffmpeg;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn encode_sample_with_mock_ffmpeg() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let mock_ffmpeg = write_mock_ffmpeg(dir.path());

        let sample_ts = dir.path().join("sample.ts");
        std::fs::write(&sample_ts, b"fake sample data").unwrap();

        let encoder = EncoderConfig::libx264();
        let output = dir.path().join("encoded.mkv");

        // Act
        let result = encode_sample(
            &mock_ffmpeg,
            &sample_ts,
            "null",
            &encoder,
            25.0,
            &[],
            &[],
            &output,
        )
        .unwrap();

        // Assert
        assert!(result.encoded_size > 0, "encoded size should be non-zero");
        assert!(result.path.exists(), "encoded file should exist");
    }
}
