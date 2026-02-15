//! TMDB API client module.
//!
//! Handles HTTP requests to the TMDB API v3 endpoints
//! and retrieves TV series, movie, and season data.

mod api;
mod client;
mod rate_limiter;
mod types;

#[allow(clippy::module_name_repetitions)]
pub use api::{LocalTmdbApi, TmdbApi};
#[allow(clippy::module_name_repetitions)]
pub use client::{TmdbClient, TmdbClientBuilder};
#[allow(clippy::module_name_repetitions)]
pub use types::{
    SearchMovieParams, SearchTvParams, TmdbMovieSearchResult, TmdbSearchMovieResponse,
    TmdbSearchTvResponse, TmdbTvDetails, TmdbTvSearchResult, TmdbTvSeason,
};
