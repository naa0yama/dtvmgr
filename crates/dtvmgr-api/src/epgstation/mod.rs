//! `EPGStation` API client module.
//!
//! Handles HTTP requests to the local `EPGStation` API endpoints
//! and manages recorded programs and encode queue operations.

mod api;
mod client;
mod rate_limiter;
mod types;

#[allow(clippy::module_name_repetitions)]
pub use api::{EpgStationApi, LocalEpgStationApi};
#[allow(clippy::module_name_repetitions)]
pub use client::{EpgStationClient, EpgStationClientBuilder};
#[allow(clippy::module_name_repetitions)]
pub use types::{
    Channel, DropLogFile, EncodeInfoResponse, EncodePreset, EncodeProgramItem, EncodeRequest,
    EncodeResponse, EpgConfig, RecordedDir, RecordedItem, RecordedParams, RecordedResponse,
    VideoFile,
};
