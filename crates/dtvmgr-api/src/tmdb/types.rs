//! TMDB API response types and search parameters.

use serde::Deserialize;

// --- Search TV ---

/// Response from `search/tv` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbSearchTvResponse {
    /// Current page number.
    pub page: u32,
    /// Search results.
    pub results: Vec<TmdbTvSearchResult>,
    /// Total number of pages.
    pub total_pages: u32,
    /// Total number of results.
    pub total_results: u32,
}

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

// --- Search Movie ---

/// Response from `search/movie` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct TmdbSearchMovieResponse {
    /// Current page number.
    pub page: u32,
    /// Search results.
    pub results: Vec<TmdbMovieSearchResult>,
    /// Total number of pages.
    pub total_pages: u32,
    /// Total number of results.
    pub total_results: u32,
}

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

// --- Search Parameters ---

/// Parameters for `search/tv` endpoint.
#[derive(Debug, Clone)]
pub struct SearchTvParams {
    /// Search query (required).
    pub query: String,
    /// Response language (default: "en-US").
    pub language: String,
    /// Result page (1-500, default: 1).
    pub page: u32,
    /// Filter by first air date year.
    pub first_air_date_year: Option<u32>,
    /// Filter by year (searches first air date and episode air dates).
    pub year: Option<u32>,
    /// Include adult content.
    pub include_adult: bool,
}

impl SearchTvParams {
    /// Creates new search params with the given query.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            language: String::from("en-US"),
            page: 1,
            first_air_date_year: None,
            year: None,
            include_adult: false,
        }
    }

    /// Sets the response language.
    #[must_use]
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    /// Sets the first air date year filter.
    #[must_use]
    pub const fn first_air_date_year(mut self, year: u32) -> Self {
        self.first_air_date_year = Some(year);
        self
    }

    /// Sets the year filter.
    #[must_use]
    pub const fn year(mut self, year: u32) -> Self {
        self.year = Some(year);
        self
    }
}

/// Parameters for `search/movie` endpoint.
#[derive(Debug, Clone)]
pub struct SearchMovieParams {
    /// Search query (required).
    pub query: String,
    /// Response language (default: "en-US").
    pub language: String,
    /// Result page (1-500, default: 1).
    pub page: u32,
    /// Filter by primary release year.
    pub primary_release_year: Option<u32>,
    /// Filter by year.
    pub year: Option<u32>,
    /// Region filter (ISO 3166-1).
    pub region: Option<String>,
    /// Include adult content.
    pub include_adult: bool,
}

impl SearchMovieParams {
    /// Creates new search params with the given query.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            language: String::from("en-US"),
            page: 1,
            primary_release_year: None,
            year: None,
            region: None,
            include_adult: false,
        }
    }

    /// Sets the response language.
    #[must_use]
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    /// Sets the year filter.
    #[must_use]
    pub const fn year(mut self, year: u32) -> Self {
        self.year = Some(year);
        self
    }
}
