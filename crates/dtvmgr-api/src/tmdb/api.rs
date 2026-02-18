//! `TmdbApi` trait definition.
#![allow(clippy::future_not_send)]

use anyhow::Result;

use super::types::{
    SearchMultiParams, TmdbAlternativeTitlesResponse, TmdbGenreListResponse, TmdbMediaType,
    TmdbSearchMultiResponse, TmdbTvDetails, TmdbTvSeason,
};

/// TMDB API trait.
///
/// Abstracts API operations for mock substitution in tests.
/// Uses `trait_variant::make` to generate a `Send`-bound async trait.
#[allow(clippy::module_name_repetitions)]
#[trait_variant::make(TmdbApi: Send)]
pub trait LocalTmdbApi {
    /// Searches for TV, movies, and people in a single request.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn search_multi(&self, params: &SearchMultiParams) -> Result<TmdbSearchMultiResponse>;

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

    /// Fetches the TV genre list.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn genre_tv_list(&self, language: &str) -> Result<TmdbGenreListResponse>;

    /// Fetches the movie genre list.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn genre_movie_list(&self, language: &str) -> Result<TmdbGenreListResponse>;

    /// Fetches alternative titles for a TV series or movie.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or JSON parsing fails.
    async fn alternative_titles(
        &self,
        media_type: TmdbMediaType,
        id: u64,
    ) -> Result<TmdbAlternativeTitlesResponse>;
}
