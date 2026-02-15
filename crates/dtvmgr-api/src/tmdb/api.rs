//! `TmdbApi` trait definition.
#![allow(clippy::future_not_send)]

use anyhow::Result;

use super::types::{
    SearchMovieParams, SearchTvParams, TmdbSearchMovieResponse, TmdbSearchTvResponse,
    TmdbTvDetails, TmdbTvSeason,
};

/// TMDB API trait.
///
/// Abstracts API operations for mock substitution in tests.
/// Uses `trait_variant::make` to generate a `Send`-bound async trait.
#[allow(clippy::module_name_repetitions)]
#[trait_variant::make(TmdbApi: Send)]
pub trait LocalTmdbApi {
    /// Searches for TV series.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn search_tv(&self, params: &SearchTvParams) -> Result<TmdbSearchTvResponse>;

    /// Searches for movies.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn search_movie(&self, params: &SearchMovieParams) -> Result<TmdbSearchMovieResponse>;

    /// Fetches TV series details including season list.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn tv_details(&self, series_id: u64, language: &str) -> Result<TmdbTvDetails>;

    /// Fetches TV season details including episode list.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn tv_season(
        &self,
        series_id: u64,
        season_number: u32,
        language: &str,
    ) -> Result<TmdbTvSeason>;
}
