//! Core types for the jlse CM detection pipeline.

use std::path::PathBuf;

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
    /// Path to JL directory containing command files and `data/`.
    pub jl_dir: PathBuf,
    /// Path to logo directory containing `.lgd` files.
    pub logo_dir: PathBuf,
    /// Path to result output directory.
    pub result_dir: PathBuf,
}
