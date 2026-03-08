//! TMDB API response types and search parameters.

use serde::{Deserialize, Serialize};

// --- Search TV Result ---

/// A single TV series search result.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbTvSearchResult {
    /// TMDB series ID.
    pub id: u64,
    /// Localized name.
    pub name: String,
    /// Original name.
    pub original_name: String,
    /// Original language (ISO 639-1).
    pub original_language: String,
    /// Origin countries (ISO 3166-1).
    pub origin_country: Vec<String>,
    /// First air date (YYYY-MM-DD or null).
    pub first_air_date: Option<String>,
    /// Overview text.
    pub overview: Option<String>,
    /// Popularity score.
    pub popularity: f64,
    /// Vote average.
    pub vote_average: f64,
    /// Vote count.
    pub vote_count: u32,
    /// Genre IDs.
    pub genre_ids: Vec<u32>,
    /// Adult flag.
    pub adult: bool,
    /// Poster image path.
    pub poster_path: Option<String>,
    /// Backdrop image path.
    pub backdrop_path: Option<String>,
}

// --- Search Movie Result ---

/// A single movie search result.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbMovieSearchResult {
    /// TMDB movie ID.
    pub id: u64,
    /// Localized title.
    pub title: String,
    /// Original title.
    pub original_title: String,
    /// Original language (ISO 639-1).
    pub original_language: String,
    /// Release date (YYYY-MM-DD or null).
    pub release_date: Option<String>,
    /// Overview text.
    pub overview: Option<String>,
    /// Popularity score.
    pub popularity: f64,
    /// Vote average.
    pub vote_average: f64,
    /// Vote count.
    pub vote_count: u32,
    /// Genre IDs.
    pub genre_ids: Vec<u32>,
    /// Adult flag.
    pub adult: bool,
    /// Video flag.
    pub video: bool,
    /// Poster image path.
    pub poster_path: Option<String>,
    /// Backdrop image path.
    pub backdrop_path: Option<String>,
}

// --- TV Details ---

/// Response from `tv/{series_id}` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbTvDetails {
    /// TMDB series ID.
    pub id: u64,
    /// Localized name.
    pub name: String,
    /// Original name.
    pub original_name: String,
    /// Original language (ISO 639-1).
    pub original_language: String,
    /// Origin countries (ISO 3166-1).
    pub origin_country: Vec<String>,
    /// First air date.
    pub first_air_date: Option<String>,
    /// Last air date.
    pub last_air_date: Option<String>,
    /// Total number of episodes.
    pub number_of_episodes: u32,
    /// Total number of seasons.
    pub number_of_seasons: u32,
    /// Season summaries.
    pub seasons: Vec<TmdbSeasonSummary>,
    /// Status (e.g., "Returning Series", "Ended").
    pub status: Option<String>,
    /// Overview text.
    pub overview: Option<String>,
    /// Popularity score.
    pub popularity: f64,
    /// Vote average.
    pub vote_average: f64,
    /// Genres.
    pub genres: Vec<TmdbGenre>,
    /// Whether the show is still in production.
    pub in_production: bool,
    /// Poster image path.
    pub poster_path: Option<String>,
}

/// Season summary within TV details.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbSeasonSummary {
    /// TMDB season ID.
    pub id: u64,
    /// Season number (0 = specials).
    pub season_number: u32,
    /// Number of episodes in this season.
    pub episode_count: u32,
    /// Air date of this season.
    pub air_date: Option<String>,
    /// Season name.
    pub name: String,
    /// Season overview.
    pub overview: Option<String>,
    /// Vote average.
    pub vote_average: f64,
}

/// Genre entry.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbGenre {
    /// Genre ID.
    pub id: u32,
    /// Genre name.
    pub name: String,
}

// --- Genre List ---

/// Response from `genre/tv/list` or `genre/movie/list` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbGenreListResponse {
    /// Genre entries.
    pub genres: Vec<TmdbGenre>,
}

// --- Search Multi ---

/// TMDB media type for multi-search and alternative titles endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TmdbMediaType {
    /// TV series.
    Tv,
    /// Movie.
    Movie,
}

impl TmdbMediaType {
    /// Returns the API path segment (e.g. "tv", "movie").
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tv => "tv",
            Self::Movie => "movie",
        }
    }
}

/// Parameters for `search/multi` endpoint.
#[derive(Debug, Clone)]
pub struct SearchMultiParams {
    /// Search query (required).
    pub query: String,
    /// Response language (default: "en-US").
    pub language: String,
    /// Result page (1-500, default: 1).
    pub page: u32,
    /// Include adult content.
    pub include_adult: bool,
}

impl SearchMultiParams {
    /// Creates new search params with the given query.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            language: String::from("en-US"),
            page: 1,
            include_adult: false,
        }
    }

    /// Sets the response language.
    #[must_use]
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    /// Sets the result page.
    #[must_use]
    pub const fn page(mut self, page: u32) -> Self {
        self.page = page;
        self
    }
}

/// Response from `search/multi` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbSearchMultiResponse {
    /// Current page number.
    pub page: u32,
    /// Search results (mixed TV, movie, person).
    pub results: Vec<TmdbMultiSearchResult>,
    /// Total number of pages.
    pub total_pages: u32,
    /// Total number of results.
    pub total_results: u32,
}

/// A single multi-search result, tagged by `media_type`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "media_type")]
pub enum TmdbMultiSearchResult {
    /// TV series result.
    #[serde(rename = "tv")]
    Tv(TmdbTvSearchResult),
    /// Movie result.
    #[serde(rename = "movie")]
    Movie(TmdbMovieSearchResult),
    /// Person result (ignored in lookup).
    #[serde(rename = "person")]
    Person(TmdbPersonSearchResult),
}

/// A person search result (only `id` is needed).
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbPersonSearchResult {
    /// TMDB person ID.
    pub id: u64,
}

// --- Alternative Titles ---

/// Response from `{media_type}/{id}/alternative_titles` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbAlternativeTitlesResponse {
    /// TMDB ID.
    pub id: u64,
    /// Alternative title entries.
    /// TV uses "results" key, movie uses "titles" key.
    #[serde(alias = "titles")]
    pub results: Vec<TmdbAlternativeTitle>,
}

/// A single alternative title entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TmdbAlternativeTitle {
    /// Country code (ISO 3166-1).
    pub iso_3166_1: String,
    /// Alternative title.
    pub title: String,
    /// Title type (e.g., "romaji").
    #[serde(rename = "type")]
    pub title_type: String,
}

// --- TV Season Details ---

/// Response from `tv/{series_id}/season/{season_number}` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbTvSeason {
    /// Internal `MongoDB` ID.
    #[serde(rename = "_id", default)]
    pub internal_id: Option<String>,
    /// TMDB season ID.
    pub id: u64,
    /// Season number.
    pub season_number: u32,
    /// Season name.
    pub name: Option<String>,
    /// Season overview.
    pub overview: Option<String>,
    /// Air date.
    pub air_date: Option<String>,
    /// Episodes in this season.
    pub episodes: Vec<TmdbEpisode>,
    /// Vote average.
    pub vote_average: f64,
}

/// A single episode within a season.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbEpisode {
    /// TMDB episode ID.
    pub id: u64,
    /// Episode number within the season.
    pub episode_number: u32,
    /// Episode name.
    pub name: String,
    /// Episode overview.
    pub overview: Option<String>,
    /// Air date.
    pub air_date: Option<String>,
    /// Season number.
    pub season_number: u32,
    /// Parent show ID.
    pub show_id: u64,
    /// Runtime in minutes.
    pub runtime: Option<u32>,
    /// Vote average.
    pub vote_average: f64,
    /// Episode type (e.g., "standard", "finale").
    pub episode_type: Option<String>,
}

// --- Error Response ---

/// TMDB API error response body.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbErrorResponse {
    /// TMDB error code.
    pub status_code: u32,
    /// Error message.
    pub status_message: String,
    /// Success flag (always false for errors).
    #[allow(dead_code)]
    pub success: bool,
}
