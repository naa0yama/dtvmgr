//! Core types for the jlse CM detection pipeline.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Broadcast channel entry from `ChList.csv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    /// Recognition name (e.g. full-width Japanese like "ＮＨＫＢＳ１").
    pub recognize: String,
    /// Installation name (usually empty).
    pub install: String,
    /// Short code used for logo lookup and param matching (e.g. `"BS1"`).
    pub short: String,
    /// Service ID as a string (e.g. `"101"`).
    pub service_id: String,
}

/// Raw parameter entry from `ChParamJL1.csv` / `ChParamJL2.csv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// Station abbreviation (matches `Channel::short`).
    pub channel: String,
    /// Title pattern (substring or regex).
    pub title: String,
    /// JL command file name (e.g. `"JL_NHK.txt"`).
    pub jl_run: String,
    /// Flag string (e.g. `"fLOff,fHCWOWA"`). `"@"` means clear.
    pub flags: String,
    /// Additional `join_logo_scp` options.
    pub options: String,
    /// Display comment.
    pub comment_view: String,
    /// Internal comment.
    pub comment: String,
}

/// Merged detection result from channel + filename matching.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectionParam {
    /// JL command file name.
    pub jl_run: String,
    /// Flag string.
    pub flags: String,
    /// Additional options.
    pub options: String,
}

/// Configuration for the jlse CM detection pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JlseConfig {
    /// Directory paths (JL, logo, result).
    pub dirs: JlseDirs,
    /// Binary path overrides. Omit to use defaults.
    #[serde(default)]
    pub bins: JlseBins,
    /// Encode settings for the `FFmpeg` step.
    #[serde(default)]
    pub encode: Option<JlseEncode>,
}

/// Encode configuration for the `FFmpeg` step.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct JlseEncode {
    /// Output container format extension (default: `"mkv"`).
    pub format: Option<String>,
    /// Input processing flags.
    #[serde(default)]
    pub input: Option<EncodeInput>,
    /// Video encoding settings.
    #[serde(default)]
    pub video: Option<EncodeVideo>,
    /// Audio encoding settings.
    #[serde(default)]
    pub audio: Option<EncodeAudio>,
}

/// `FFmpeg` input processing flags.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EncodeInput {
    /// `-fflags` value (e.g. `"+discardcorrupt+genpts"`).
    pub flags: Option<String>,
    /// `-analyzeduration` value (e.g. `"30M"`).
    pub analyzeduration: Option<String>,
    /// `-probesize` value (e.g. `"100M"`).
    pub probesize: Option<String>,
}

/// `FFmpeg` video encoding settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EncodeVideo {
    /// `-c:v` codec name (e.g. `"hevc_nvenc"`).
    pub codec: Option<String>,
    /// `-preset` value (e.g. `"slow"`).
    pub preset: Option<String>,
    /// `-profile:v` value (e.g. `"main10"`).
    pub profile: Option<String>,
    /// `-pix_fmt` value (e.g. `"yuv420p10le"`).
    pub pix_fmt: Option<String>,
    /// `-aspect` value (e.g. `"16:9"`).
    pub aspect: Option<String>,
    /// `-vf` filter string (e.g. `"yadif=...,scale=..."`).
    pub filter: Option<String>,
    /// Additional freeform video options as key-value pairs.
    /// Each element is appended as-is (e.g. `["-rc:v", "constqp", "-g", "250"]`).
    #[serde(default)]
    pub extra: Vec<String>,
}

/// `FFmpeg` audio encoding settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EncodeAudio {
    /// `-c:a` codec name (e.g. `"aac"`).
    pub codec: Option<String>,
    /// `-ar` sample rate (e.g. `48000`).
    pub sample_rate: Option<u32>,
    /// `-ab` bitrate (e.g. `"256k"`).
    pub bitrate: Option<String>,
    /// `-ac` channel count (e.g. `2`).
    pub channels: Option<u32>,
    /// Additional freeform audio options.
    /// Each element is appended as-is.
    #[serde(default)]
    pub extra: Vec<String>,
}

/// Required directory paths for the pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JlseDirs {
    /// Path to JL directory containing command files and `data/`.
    pub jl: PathBuf,
    /// Path to logo directory containing `.lgd` files.
    pub logo: PathBuf,
    /// Path to result output directory.
    pub result: PathBuf,
}

impl JlseDirs {
    /// Derive the default binary directory from the JL path.
    ///
    /// Returns `<jl_parent>/bin/` — the conventional location for
    /// JL-bundled binaries.
    #[must_use]
    pub fn bin_dir(&self) -> PathBuf {
        self.jl
            .parent()
            .unwrap_or_else(|| Path::new("/"))
            .join("bin")
    }
}

/// Encode target AVS selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AvsTarget {
    /// Cut CM only (`in_cutcm.avs`).
    CutCm,
    /// Cut CM + logo removal (`in_cutcm_logo.avs`).
    #[default]
    CutCmLogo,
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn test_jlse_encode_deserialize_full() {
        // Arrange
        let toml_str = r#"
format = "mkv"

[input]
flags = "+discardcorrupt+genpts"
analyzeduration = "30M"
probesize = "100M"

[video]
codec = "hevc_nvenc"
preset = "slow"
profile = "main10"
pix_fmt = "yuv420p10le"
filter = "yadif=mode=send_frame"
extra = ["-rc:v", "constqp", "-g", "250"]

[audio]
codec = "aac"
sample_rate = 48000
bitrate = "256k"
channels = 2
"#;

        // Act
        let encode: JlseEncode = toml::from_str(toml_str).unwrap();

        // Assert
        assert_eq!(encode.format.as_deref(), Some("mkv"));
        let input = encode.input.unwrap();
        assert_eq!(input.flags.as_deref(), Some("+discardcorrupt+genpts"));
        assert_eq!(input.analyzeduration.as_deref(), Some("30M"));
        assert_eq!(input.probesize.as_deref(), Some("100M"));
        let video = encode.video.unwrap();
        assert_eq!(video.codec.as_deref(), Some("hevc_nvenc"));
        assert_eq!(video.preset.as_deref(), Some("slow"));
        assert_eq!(video.profile.as_deref(), Some("main10"));
        assert_eq!(video.pix_fmt.as_deref(), Some("yuv420p10le"));
        assert_eq!(video.filter.as_deref(), Some("yadif=mode=send_frame"));
        assert_eq!(video.extra, vec!["-rc:v", "constqp", "-g", "250"]);
        let audio = encode.audio.unwrap();
        assert_eq!(audio.codec.as_deref(), Some("aac"));
        assert_eq!(audio.sample_rate, Some(48000));
        assert_eq!(audio.bitrate.as_deref(), Some("256k"));
        assert_eq!(audio.channels, Some(2));
    }

    #[test]
    fn test_jlse_encode_deserialize_empty() {
        // Arrange / Act
        let encode: JlseEncode = toml::from_str("").unwrap();

        // Assert
        assert_eq!(encode, JlseEncode::default());
    }

    #[test]
    fn test_jlse_encode_deserialize_partial() {
        // Arrange
        let toml_str = r#"
[video]
codec = "libx264"
"#;

        // Act
        let encode: JlseEncode = toml::from_str(toml_str).unwrap();

        // Assert
        assert!(encode.format.is_none());
        assert!(encode.input.is_none());
        assert_eq!(encode.video.unwrap().codec.as_deref(), Some("libx264"));
        assert!(encode.audio.is_none());
    }

    #[test]
    fn test_jlse_config_with_encode_roundtrip() {
        // Arrange
        let toml_str = r#"
[dirs]
jl = "/opt/JL"
logo = "/opt/logo"
result = "/tmp/result"

[encode]
format = "mp4"

[encode.audio]
codec = "aac"
sample_rate = 44100
"#;

        // Act
        let config: JlseConfig = toml::from_str(toml_str).unwrap();

        // Assert
        assert_eq!(
            config.encode.as_ref().unwrap().format.as_deref(),
            Some("mp4")
        );
        let audio = config.encode.unwrap().audio.unwrap();
        assert_eq!(audio.codec.as_deref(), Some("aac"));
        assert_eq!(audio.sample_rate, Some(44100));
    }
}

/// Optional binary path overrides. `None` fields use default derivation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct JlseBins {
    /// logoframe binary override.
    pub logoframe: Option<PathBuf>,
    /// `chapter_exe` binary override.
    pub chapter_exe: Option<PathBuf>,
    /// `join_logo_scp` binary override.
    pub join_logo_scp: Option<PathBuf>,
    /// ffprobe binary override.
    pub ffprobe: Option<PathBuf>,
    /// ffmpeg binary override.
    pub ffmpeg: Option<PathBuf>,
    /// `tstables` binary override.
    pub tstables: Option<PathBuf>,
}
