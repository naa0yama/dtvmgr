//! `SyoboiApi` trait definition.
#![allow(clippy::future_not_send)]

use anyhow::Result;

use super::params::ProgLookupParams;
use super::types::{SyoboiChannel, SyoboiChannelGroup, SyoboiProgram, SyoboiTitle};

/// Syoboi Calendar API trait.
///
/// Abstracts API operations for mock substitution in tests.
/// Uses `trait_variant::make` to generate a `Send`-bound async trait.
#[allow(clippy::module_name_repetitions)]
#[trait_variant::make(SyoboiApi: Send)]
pub trait LocalSyoboiApi {
    /// Looks up title information.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or XML parsing fails.
    async fn lookup_titles(&self, tids: &[u32]) -> Result<Vec<SyoboiTitle>>;

    /// Looks up program data (5,000-item limit per request).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or XML parsing fails.
    async fn lookup_programs(&self, params: &ProgLookupParams) -> Result<Vec<SyoboiProgram>>;

    /// Looks up channel information.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or XML parsing fails.
    async fn lookup_channels(&self, ch_ids: Option<&[u32]>) -> Result<Vec<SyoboiChannel>>;

    /// Looks up channel group information.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or XML parsing fails.
    async fn lookup_channel_groups(
        &self,
        ch_gids: Option<&[u32]>,
    ) -> Result<Vec<SyoboiChannelGroup>>;
}
