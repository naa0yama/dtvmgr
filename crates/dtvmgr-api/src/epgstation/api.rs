//! `EpgStationApi` trait definition.
#![allow(clippy::future_not_send)]

use anyhow::Result;

use super::types::{
    Channel, EncodeInfoResponse, EncodeRequest, EncodeResponse, EpgConfig, RecordedItem,
    RecordedParams, RecordedResponse,
};

/// `EPGStation` API trait.
///
/// Abstracts API operations for mock substitution in tests.
/// Uses `trait_variant::make` to generate a `Send`-bound async trait.
#[allow(clippy::module_name_repetitions)]
#[trait_variant::make(EpgStationApi: Send)]
pub trait LocalEpgStationApi {
    /// Fetches recorded program list.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn fetch_recorded(&self, params: &RecordedParams) -> Result<RecordedResponse>;

    /// Fetches a single recorded program by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn fetch_recorded_by_id(&self, id: u64) -> Result<RecordedItem>;

    /// Fetches broadcast channel list.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn fetch_channels(&self) -> Result<Vec<Channel>>;

    /// Fetches server configuration (encode presets and recorded directories).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn fetch_config(&self) -> Result<EpgConfig>;

    /// Adds an encode job to the queue.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn add_encode(&self, body: &EncodeRequest) -> Result<EncodeResponse>;

    /// Fetches the current encode queue status.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn fetch_encode_queue(&self) -> Result<EncodeInfoResponse>;

    /// Checks whether a video file exists on the `EPGStation` server.
    ///
    /// Sends a HEAD request to `GET /api/videos/{id}`.
    /// Returns `true` if the server responds with 200, `false` otherwise.
    async fn check_video_file_exists(&self, video_file_id: u64) -> bool;
}
