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
