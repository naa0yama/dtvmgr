//! `EPGStation` API types.

use serde::{Deserialize, Serialize};

// --- Recorded ---

/// Query parameters for `GET /api/recorded`.
#[derive(Debug, Clone, Default)]
pub struct RecordedParams {
    /// Whether to include only items with original files.
    pub has_original_file: Option<bool>,
    /// Number of items to return.
    pub limit: Option<u64>,
    /// Offset for pagination.
    pub offset: Option<u64>,
    /// Whether to fetch in reverse order.
    pub is_reverse: Option<bool>,
    /// Whether to use half-width characters in responses.
    pub is_half_width: Option<bool>,
    /// Keyword filter.
    pub keyword: Option<String>,
}

/// Response from `GET /api/recorded`.
#[derive(Debug, Clone, Deserialize)]
pub struct RecordedResponse {
    /// Total number of recorded items.
    pub total: u64,
    /// Recorded items.
    pub records: Vec<RecordedItem>,
}

/// A single recorded program.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedItem {
    /// Recorded item ID.
    pub id: u64,
    /// Channel ID.
    pub channel_id: u64,
    /// Program name.
    pub name: String,
    /// Program description.
    pub description: Option<String>,
    /// Extended description.
    pub extended: Option<String>,
    /// Start timestamp (Unix ms).
    pub start_at: u64,
    /// End timestamp (Unix ms).
    pub end_at: u64,
    /// Whether the item is currently recording.
    pub is_recording: bool,
    /// Whether the item is currently encoding.
    pub is_encoding: bool,
    /// Whether the item is protected.
    pub is_protected: bool,
    /// Video resolution (e.g. "1080i").
    pub video_resolution: Option<String>,
    /// Video type (e.g. "mpeg2").
    pub video_type: Option<String>,
    /// Video files associated with this recorded item.
    #[serde(default)]
    pub video_files: Vec<VideoFile>,
    /// Drop log file information.
    pub drop_log_file: Option<DropLogFile>,
}

/// A video file entry within a recorded item.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoFile {
    /// Video file ID.
    pub id: u64,
    /// File name.
    pub name: String,
    /// File name on disk.
    pub filename: Option<String>,
    /// File type (e.g. "ts", "encoded").
    #[serde(rename = "type")]
    pub file_type: String,
    /// File size in bytes.
    pub size: u64,
}

/// Drop log file information.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DropLogFile {
    /// Drop count.
    #[serde(default)]
    pub drop_cnt: u64,
    /// Error count.
    #[serde(default)]
    pub error_cnt: u64,
    /// Scrambling count.
    #[serde(default)]
    pub scrambling_cnt: u64,
}

// --- Channel ---

/// A broadcast channel.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    /// Channel ID.
    pub id: u64,
    /// Channel name.
    pub name: String,
    /// Half-width channel name.
    pub half_width_name: String,
    /// Channel type (e.g. "GR", "BS", "CS").
    pub channel_type: String,
    /// Whether logo data exists.
    pub has_logo_data: bool,
}

// --- Config ---

/// `EPGStation` server configuration (from `GET /api/config`).
#[derive(Debug, Clone, Deserialize)]
pub struct EpgConfig {
    /// Available encode presets.
    #[serde(deserialize_with = "deserialize_encode_presets")]
    pub encode: Vec<EncodePreset>,
    /// Recorded file directories.
    #[serde(deserialize_with = "deserialize_recorded_dirs")]
    pub recorded: Vec<RecordedDir>,
}

/// An encode preset entry.
#[derive(Debug, Clone)]
pub struct EncodePreset {
    /// Preset name (used as `mode` in encode requests).
    pub name: String,
}

/// Raw encode preset (object or string).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum EncodePresetRaw {
    /// Object form: `{"name": "H.264"}`.
    Full { name: String },
    /// String form: `"H.264"`.
    NameOnly(String),
}

fn deserialize_encode_presets<'de, D>(deserializer: D) -> Result<Vec<EncodePreset>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<EncodePresetRaw>::deserialize(deserializer).map(|raw| {
        raw.into_iter()
            .map(|r| match r {
                EncodePresetRaw::Full { name } | EncodePresetRaw::NameOnly(name) => {
                    EncodePreset { name }
                }
            })
            .collect()
    })
}

/// A recorded directory entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RecordedDirRaw {
    /// Object form: `{"name": "...", "path": "..."}`.
    Full { name: String, path: String },
    /// String form: `"recorded"` (name only, path unknown).
    NameOnly(String),
}

/// A recorded directory entry.
#[derive(Debug, Clone)]
pub struct RecordedDir {
    /// Directory name.
    pub name: String,
    /// Directory path.
    pub path: String,
}

fn deserialize_recorded_dirs<'de, D>(deserializer: D) -> Result<Vec<RecordedDir>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<RecordedDirRaw>::deserialize(deserializer).map(|raw| {
        raw.into_iter()
            .map(|r| match r {
                RecordedDirRaw::Full { name, path } => RecordedDir { name, path },
                RecordedDirRaw::NameOnly(name) => RecordedDir {
                    path: name.clone(),
                    name,
                },
            })
            .collect()
    })
}

// --- Encode Request ---

/// Request body for `POST /api/encode`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodeRequest {
    /// Recorded item ID.
    pub recorded_id: u64,
    /// Source video file ID.
    pub source_video_file_id: u64,
    /// Encode preset name.
    pub mode: String,
    /// Parent directory name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_dir: Option<String>,
    /// Sub-directory within parent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    /// Whether to save in the same directory as the source.
    pub is_save_same_directory: bool,
    /// Whether to remove the original file after encoding.
    pub remove_original: bool,
}

/// Response from `POST /api/encode`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodeResponse {
    /// Encode program ID (queue entry ID).
    pub encode_program_id: u64,
}

// --- Encode Queue ---

/// Response from `GET /api/encode`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodeInfoResponse {
    /// Currently running encode items.
    pub running_items: Vec<EncodeProgramItem>,
    /// Waiting encode items.
    pub wait_items: Vec<EncodeProgramItem>,
}

/// An item in the encode queue.
#[derive(Debug, Clone, Deserialize)]
pub struct EncodeProgramItem {
    /// Encode program ID.
    pub id: u64,
    /// Encode preset name.
    pub mode: String,
    /// The recorded item being encoded.
    pub recorded: RecordedItem,
    /// Encode progress percentage (0-100).
    pub percent: Option<f64>,
}
