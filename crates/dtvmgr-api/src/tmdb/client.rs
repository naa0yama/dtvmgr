//! `TmdbClient` - TMDB API client implementation.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Client;
use reqwest::header::{AUTHORIZATION, HeaderValue};
use tokio::sync::Mutex;
use tracing::instrument;
use url::Url;

use crate::rate_limiter::SimpleRateLimiter;

use super::api::LocalTmdbApi;
use super::types::{
    SearchMultiParams, TmdbAlternativeTitlesResponse, TmdbErrorResponse, TmdbGenreListResponse,
    TmdbMediaType, TmdbSearchMultiResponse, TmdbTvDetails, TmdbTvSeason,
};

/// Default base URL for TMDB API v3.
const DEFAULT_BASE_URL: &str = "https://api.themoviedb.org/3/";

/// Maximum number of retries for HTTP 429 responses.
const MAX_RETRIES: u32 = 3;

/// Maximum number of retries for transient network errors (e.g. keep-alive race).
const MAX_NETWORK_RETRIES: u32 = 1;

/// Backoff duration between retries.
const RETRY_BACKOFF: Duration = Duration::from_secs(1);

/// Wrapper that prevents sensitive values from leaking via `Debug`.
///
/// Does not implement `Display` to avoid accidental formatting.
struct Secret(String);

impl Secret {
    /// Returns the inner value for constructing auth headers.
    fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// TMDB API client.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct TmdbClient {
    /// HTTP client.
    http_client: Client,
    /// Base URL for API requests.
    base_url: Url,
    /// Bearer API token.
    api_token: Secret,
    /// Rate limiter.
    rate_limiter: Arc<Mutex<SimpleRateLimiter>>,
}

/// Builder for `TmdbClient`.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct TmdbClientBuilder {
    base_url: Option<Url>,
    api_token: Option<String>,
    user_agent: Option<String>,
    min_interval: Option<Duration>,
}

impl TmdbClientBuilder {
    /// Creates a new builder.
    const fn new() -> Self {
        Self {
            base_url: None,
            api_token: None,
            user_agent: None,
            min_interval: None,
        }
    }

    /// Overrides the base URL (for wiremock in tests).
    #[must_use]
    pub fn base_url(mut self, url: Url) -> Self {
        self.base_url = Some(url);
        self
    }

    /// Sets the API bearer token (required).
    #[must_use]
    pub fn api_token(mut self, token: impl Into<String>) -> Self {
        self.api_token = Some(token.into());
        self
    }

    /// Sets the User-Agent (required).
    #[must_use]
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Sets the minimum request interval (default: 25ms).
    #[must_use]
    pub const fn min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = Some(interval);
        self
    }

    /// Builds the client.
    ///
    /// # Errors
    ///
    /// - `api_token` is not set.
    /// - `user_agent` is not set.
    /// - `reqwest::Client` build fails.
    pub fn build(self) -> Result<TmdbClient> {
        let api_token = self.api_token.context("api_token is required")?;
        let user_agent = self.user_agent.context("user_agent is required")?;

        let base_url = if let Some(url) = self.base_url {
            url
        } else {
            let result = Url::parse(DEFAULT_BASE_URL);
            result.context("invalid default base URL")?
        };

        let rate_limiter = self
            .min_interval
            .map_or_else(super::rate_limiter::default_limiter, |interval| {
                SimpleRateLimiter::new(interval, "tmdb")
            });

        let http_client = Client::builder()
            .user_agent(&user_agent)
            .gzip(true)
            .build()
            .context("failed to build HTTP client")?;

        Ok(TmdbClient {
            http_client,
            base_url,
            api_token: Secret(api_token),
            rate_limiter: Arc::new(Mutex::new(rate_limiter)),
        })
    }
}

impl TmdbClient {
    /// Creates a new builder.
    #[must_use]
    pub const fn builder() -> TmdbClientBuilder {
        TmdbClientBuilder::new()
    }

    /// Sends a request with rate limiting, retry on 429, and JSON parsing.
    ///
    /// TMDB-specific: parses `TmdbErrorResponse` for structured error messages.
    async fn request_with_retry<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        #[cfg_attr(not(feature = "otel"), allow(unused_variables))] method: &'static str,
        build_request: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<T> {
        self.rate_limiter.lock().await.wait().await;

        #[cfg(feature = "otel")]
        let request_start = std::time::Instant::now();
        let mut network_retries = 0u32;
        let mut rate_limit_retries = 0u32;
        loop {
            // SECURITY: build request WITHOUT auth so build errors cannot leak
            // the bearer token. Auth is injected via raw header after build.
            let mut request = build_request()
                .build()
                .with_context(|| format!("failed to build request: {path}"))?;

            // SECURITY: URL does not contain auth token (Bearer is in header)
            tracing::Span::current().record("url.full", tracing::field::display(request.url()));

            // Inject auth header separately — keeps the token out of the
            // RequestBuilder closure and limits the taint surface.
            // SECURITY: discard the InvalidHeaderValue error entirely — its
            // Display impl may echo the bearer token, breaking the taint chain.
            let Ok(auth_value) =
                HeaderValue::from_str(&format!("Bearer {}", self.api_token.expose()))
            else {
                bail!("failed to set authorization header");
            };
            request.headers_mut().insert(AUTHORIZATION, auth_value);

            let response = match self.http_client.execute(request).await {
                Ok(resp) => resp,
                Err(e) if !e.is_timeout() && network_retries < MAX_NETWORK_RETRIES => {
                    network_retries = network_retries.saturating_add(1);
                    // SECURITY: log classified kind only — reqwest::Error from
                    // execute() may carry request context; never format it.
                    let kind = crate::classify_reqwest_error(&e);
                    tracing::debug!(
                        retry = network_retries,
                        error.kind = kind,
                        "transient network error, retrying"
                    );
                    continue;
                }
                Err(e) => {
                    let kind = crate::classify_reqwest_error(&e);
                    let status_code = e.status().map(|s| i64::from(s.as_u16()));
                    if let Some(code) = status_code {
                        tracing::Span::current().record("http.response.status_code", code);
                    }
                    // SECURITY: do not format the raw reqwest::Error — it
                    // originates from execute(request) where request carried
                    // the bearer token. Only emit the classified kind.
                    bail!("{kind}: {path}");
                }
            };

            let span = tracing::Span::current();
            let status = response.status();
            span.record("http.response.status_code", i64::from(status.as_u16()));

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                #[cfg(feature = "otel")]
                crate::metrics::record_rate_limit_hit("tmdb");

                rate_limit_retries = rate_limit_retries.saturating_add(1);
                if rate_limit_retries > MAX_RETRIES {
                    bail!("TMDB API rate limit exceeded after {MAX_RETRIES} retries: {path}");
                }
                tracing::warn!(
                    retry = rate_limit_retries,
                    max_retries = MAX_RETRIES,
                    "TMDB API rate limited (429). Retrying..."
                );
                tokio::time::sleep(RETRY_BACKOFF.saturating_mul(rate_limit_retries)).await;
                self.rate_limiter.lock().await.wait().await;
                continue;
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_else(|e| {
                    let kind = crate::classify_reqwest_error(&e);
                    tracing::warn!(error.kind = kind, "failed to read error response body");
                    format!("<failed to read body: {kind}>")
                });
                span.record("http.response.body", &body);
                if let Ok(error_response) = serde_json::from_str::<TmdbErrorResponse>(&body) {
                    bail!(
                        "TMDB API error (HTTP {}): code={}, message={}",
                        status,
                        error_response.status_code,
                        error_response.status_message,
                    );
                }
                bail!("TMDB API error (HTTP {status}): {body}");
            }

            // SECURITY: use map_err instead of with_context to discard the
            // original reqwest/serde errors — they sit in a taint chain rooted
            // at the Bearer header injected into `request`.  Dropping them
            // breaks the CWE-532 taint path that CodeQL tracks.
            // Extract only safe primitive labels (classify / Category / line / column).
            let body = response.text().await.map_err(|e| {
                let kind = crate::classify_reqwest_error(&e);
                anyhow::anyhow!("failed to read response body: {path} ({kind})")
            })?;
            span.record("http.response.body", body.as_str());
            let parsed: T = serde_json::from_str(&body)
                .map_err(|e| {
                    let cat = e.classify();
                    anyhow::anyhow!(
                        "decode error ({cat:?}) at line {}, column {}",
                        e.line(),
                        e.column(),
                    )
                })
                .with_context(|| format!("failed to decode JSON response: {path}"))?;

            #[cfg(feature = "otel")]
            crate::metrics::record_request_duration("tmdb", method, request_start);

            return Ok(parsed);
        }
    }

    /// Sends a GET request with Bearer auth, query params, and rate limiting.
    #[instrument(skip_all, fields(
        otel.kind = "Client",
        http.request.method = "GET",
        url.path = path,
        url.full = tracing::field::Empty,
        http.response.status_code = tracing::field::Empty,
        http.response.body = tracing::field::Empty,
    ), err(level = "warn"))]
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let url = self
            .base_url
            .join(path)
            .with_context(|| format!("failed to join URL path: {path}"))?;

        self.request_with_retry(path, "GET", || {
            self.http_client.get(url.clone()).query(query)
        })
        .await
    }
}

impl LocalTmdbApi for TmdbClient {
    #[instrument(skip_all, err(level = "error"))]
    async fn tv_details(&self, series_id: u64, language: &str) -> Result<TmdbTvDetails> {
        let path = format!("tv/{series_id}");
        let query = [("language", String::from(language))];
        self.get_json(&path, &query).await
    }

    #[instrument(skip_all, err(level = "error"))]
    async fn tv_season(
        &self,
        series_id: u64,
        season_number: u32,
        language: &str,
    ) -> Result<TmdbTvSeason> {
        let path = format!("tv/{series_id}/season/{season_number}");
        let query = [("language", String::from(language))];
        self.get_json(&path, &query).await
    }

    #[instrument(skip_all, err(level = "error"))]
    async fn genre_tv_list(&self, language: &str) -> Result<TmdbGenreListResponse> {
        self.get_json("genre/tv/list", &[("language", String::from(language))])
            .await
    }

    #[instrument(skip_all, err(level = "error"))]
    async fn genre_movie_list(&self, language: &str) -> Result<TmdbGenreListResponse> {
        self.get_json("genre/movie/list", &[("language", String::from(language))])
            .await
    }

    #[instrument(skip_all, err(level = "error"))]
    async fn search_multi(&self, params: &SearchMultiParams) -> Result<TmdbSearchMultiResponse> {
        let query: Vec<(&str, String)> = vec![
            ("query", params.query.clone()),
            ("language", params.language.clone()),
            ("page", params.page.to_string()),
            ("include_adult", params.include_adult.to_string()),
        ];
        self.get_json("search/multi", &query).await
    }

    #[instrument(skip_all, err(level = "error"))]
    async fn alternative_titles(
        &self,
        media_type: TmdbMediaType,
        id: u64,
    ) -> Result<TmdbAlternativeTitlesResponse> {
        let path = format!("{}/{id}/alternative_titles", media_type.as_str());
        self.get_json(&path, &[]).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]
    #![allow(clippy::panic)]

    use super::super::types::TmdbMultiSearchResult;
    use super::*;

    #[test]
    fn test_builder_requires_api_token() {
        // Arrange & Act
        let result = TmdbClient::builder().user_agent("test/0.0.0").build();

        // Assert
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("api_token is required")
        );
    }

    #[test]
    fn test_builder_requires_user_agent() {
        // Arrange & Act
        let result = TmdbClient::builder().api_token("test-token").build();

        // Assert
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("user_agent is required")
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_builder_with_required_fields_succeeds() {
        // Arrange & Act
        let result = TmdbClient::builder()
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .build();

        // Assert
        assert!(result.is_ok());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_builder_with_custom_base_url() {
        // Arrange
        let custom_url = Url::parse("http://localhost:8080/3/").unwrap();

        // Act
        let client = TmdbClient::builder()
            .base_url(custom_url.clone())
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .build()
            .unwrap();

        // Assert
        assert_eq!(client.base_url, custom_url);
    }

    #[test]
    fn test_parse_tv_details_fixture() {
        // Arrange
        let json = include_str!("../../../../fixtures/tmdb/tv_details_120089.json");

        // Act
        let details: TmdbTvDetails = serde_json::from_str(json).unwrap();

        // Assert
        assert_eq!(details.id, 120_089);
        assert_eq!(details.name, "SPY×FAMILY");
        assert_eq!(details.original_language, "ja");
        assert!(!details.seasons.is_empty());
        assert!(details.number_of_seasons >= 2);
    }

    #[test]
    fn test_parse_tv_season_fixture() {
        // Arrange
        let json = include_str!("../../../../fixtures/tmdb/tv_season_120089_1.json");

        // Act
        let season: TmdbTvSeason = serde_json::from_str(json).unwrap();

        // Assert
        assert_eq!(season.season_number, 1);
        assert!(!season.episodes.is_empty());
        let first_ep = &season.episodes[0];
        assert_eq!(first_ep.episode_number, 1);
        assert_eq!(first_ep.season_number, 1);
    }

    #[test]
    fn test_parse_error_response() {
        // Arrange
        let json = r#"{"status_code":7,"status_message":"Invalid API key: You must be granted a valid key.","success":false}"#;

        // Act
        let error: TmdbErrorResponse = serde_json::from_str(json).unwrap();

        // Assert
        assert_eq!(error.status_code, 7);
        assert!(!error.success);
        assert!(error.status_message.contains("Invalid API key"));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_tv_details_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body = include_str!("../../../../fixtures/tmdb/tv_details_120089.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/3/tv/120089"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let details = client.tv_details(120_089, "ja-JP").await.unwrap();

        // Assert
        assert_eq!(details.id, 120_089);
        assert_eq!(details.name, "SPY×FAMILY");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_tv_season_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body = include_str!("../../../../fixtures/tmdb/tv_season_120089_1.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/3/tv/120089/season/1"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let season = client.tv_season(120_089, 1, "ja-JP").await.unwrap();

        // Assert
        assert_eq!(season.season_number, 1);
        assert!(!season.episodes.is_empty());
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_bearer_token_is_sent() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body = include_str!("../../../../fixtures/tmdb/search_multi_empty.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/3/search/multi"))
            .and(wiremock::matchers::header(
                "Authorization",
                "Bearer my-secret-token",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .expect(1)
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("my-secret-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        let params = SearchMultiParams::new("test");

        // Act & Assert (mock expect(1) verifies Authorization header)
        client.search_multi(&params).await.unwrap();
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_http_error_returns_tmdb_error() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let error_body = r#"{"status_code":7,"status_message":"Invalid API key: You must be granted a valid key.","success":false}"#;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/3/search/multi"))
            .respond_with(wiremock::ResponseTemplate::new(401).set_body_string(error_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("invalid-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        let params = SearchMultiParams::new("test");

        // Act
        let result = client.search_multi(&params).await;

        // Assert
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("TMDB API error"));
        assert!(err.contains("Invalid API key"));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_http_429_retries() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let error_body = r#"{"status_code":25,"status_message":"Your request count is over the allowed limit.","success":false}"#;

        // Return 429 for all requests — expect retries + initial = MAX_RETRIES + 1
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/3/search/multi"))
            .respond_with(wiremock::ResponseTemplate::new(429).set_body_string(error_body))
            .expect(u64::from(MAX_RETRIES) + 1)
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        let params = SearchMultiParams::new("test");

        // Act
        let result = client.search_multi(&params).await;

        // Assert
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rate limit"));
    }

    #[test]
    fn test_parse_genre_tv_list_fixture() {
        // Arrange
        let json = include_str!("../../../../fixtures/tmdb/genre_tv_list.json");

        // Act
        let response: TmdbGenreListResponse = serde_json::from_str(json).unwrap();

        // Assert
        assert!(!response.genres.is_empty());
        let animation = response.genres.iter().find(|g| g.id == 16);
        assert!(animation.is_some());
        assert_eq!(animation.unwrap().name, "アニメーション");
    }

    #[test]
    fn test_parse_tv_alternative_titles_fixture() {
        // Arrange
        let json = include_str!("../../../../fixtures/tmdb/tv_alternative_titles_31572.json");

        // Act
        let response: TmdbAlternativeTitlesResponse = serde_json::from_str(json).unwrap();

        // Assert
        assert_eq!(response.id, 31572);
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.results[0].title, "Lupin III");
        assert_eq!(response.results[0].iso_3166_1, "JP");
        assert_eq!(response.results[0].title_type, "romaji");
        assert_eq!(response.results[1].title, "Rupan Sansei");
    }

    #[test]
    fn test_alternative_title_serializes() {
        // Arrange
        let json = include_str!("../../../../fixtures/tmdb/tv_alternative_titles_31572.json");
        let response: TmdbAlternativeTitlesResponse = serde_json::from_str(json).unwrap();

        // Act
        let serialized = serde_json::to_string(&response.results).unwrap();

        // Assert
        assert!(serialized.contains("Lupin III"));
        assert!(serialized.contains("romaji"));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_genre_tv_list_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body = include_str!("../../../../fixtures/tmdb/genre_tv_list.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/3/genre/tv/list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let response = client.genre_tv_list("ja").await.unwrap();

        // Assert
        assert!(!response.genres.is_empty());
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_alternative_titles_tv_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body = include_str!("../../../../fixtures/tmdb/tv_alternative_titles_31572.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/3/tv/31572/alternative_titles"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let response = client
            .alternative_titles(TmdbMediaType::Tv, 31572)
            .await
            .unwrap();

        // Assert
        assert_eq!(response.id, 31572);
        assert_eq!(response.results.len(), 2);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_alternative_titles_movie_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body =
            include_str!("../../../../fixtures/tmdb/movie_alternative_titles_916224.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(
                "/3/movie/916224/alternative_titles",
            ))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let response = client
            .alternative_titles(TmdbMediaType::Movie, 916_224)
            .await
            .unwrap();

        // Assert
        assert_eq!(response.id, 916_224);
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.results[0].title, "すずめの戸締まり");
    }

    #[test]
    fn test_parse_search_multi_fixture() {
        // Arrange
        let json = include_str!("../../../../fixtures/tmdb/search_multi_lupin.json");

        // Act
        let response: TmdbSearchMultiResponse = serde_json::from_str(json).unwrap();

        // Assert
        assert_eq!(response.page, 1);
        assert_eq!(response.total_results, 2);
        assert_eq!(response.results.len(), 2);

        match &response.results[0] {
            TmdbMultiSearchResult::Tv(tv) => {
                assert_eq!(tv.id, 31572);
                assert_eq!(tv.original_name, "ルパン三世");
            }
            _ => panic!("expected Tv variant"),
        }

        match &response.results[1] {
            TmdbMultiSearchResult::Movie(movie) => {
                assert_eq!(movie.id, 916_224);
                assert_eq!(movie.original_title, "ルパン三世 THE FIRST");
            }
            _ => panic!("expected Movie variant"),
        }
    }

    #[test]
    fn test_parse_search_multi_empty_fixture() {
        // Arrange
        let json = include_str!("../../../../fixtures/tmdb/search_multi_empty.json");

        // Act
        let response: TmdbSearchMultiResponse = serde_json::from_str(json).unwrap();

        // Assert
        assert_eq!(response.total_results, 0);
        assert!(response.results.is_empty());
    }

    #[test]
    fn test_parse_search_multi_with_person() {
        // Arrange
        let json = include_str!("../../../../fixtures/tmdb/search_multi_with_person.json");

        // Act
        let response: TmdbSearchMultiResponse = serde_json::from_str(json).unwrap();

        // Assert
        assert_eq!(response.results.len(), 2);

        match &response.results[0] {
            TmdbMultiSearchResult::Tv(tv) => assert_eq!(tv.id, 31572),
            _ => panic!("expected Tv variant"),
        }

        match &response.results[1] {
            TmdbMultiSearchResult::Person(person) => assert_eq!(person.id, 12345),
            _ => panic!("expected Person variant"),
        }
    }

    #[test]
    fn test_parse_movie_alternative_titles_fixture() {
        // Arrange
        let json = include_str!("../../../../fixtures/tmdb/movie_alternative_titles_916224.json");

        // Act
        let response: TmdbAlternativeTitlesResponse = serde_json::from_str(json).unwrap();

        // Assert
        assert_eq!(response.id, 916_224);
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.results[0].title, "すずめの戸締まり");
        assert_eq!(response.results[1].title, "Suzume no Tojimari");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_search_multi_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body = include_str!("../../../../fixtures/tmdb/search_multi_lupin.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/3/search/multi"))
            .and(wiremock::matchers::header_exists("Authorization"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        let params = SearchMultiParams::new("ルパン三世").language("ja-JP");

        // Act
        let response = client.search_multi(&params).await.unwrap();

        // Assert
        assert_eq!(response.results.len(), 2);
    }

    #[test]
    fn test_secret_debug_redacted() {
        // Arrange
        let secret = Secret(String::from("my-super-secret-token"));

        // Act
        let debug_str = format!("{secret:?}");

        // Assert: value is redacted
        assert_eq!(debug_str, "[REDACTED]");
        assert!(!debug_str.contains("my-super-secret-token"));
    }

    #[test]
    fn test_secret_expose() {
        // Arrange
        let secret = Secret(String::from("my-token"));

        // Act
        let exposed = secret.expose();

        // Assert
        assert_eq!(exposed, "my-token");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_http_error_non_json_body() {
        // Arrange: non-JSON error body falls through to generic error
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/3/tv/99999"))
            .respond_with(
                wiremock::ResponseTemplate::new(500).set_body_string("Internal Server Error"),
            )
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let result = client.tv_details(99999, "ja-JP").await;

        // Assert
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("TMDB API error"),
            "expected 'TMDB API error' in: {err}"
        );
        assert!(
            err.contains("Internal Server Error"),
            "expected body in error: {err}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_json_decode_error() {
        // Arrange: server returns invalid JSON with 200 status
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/3/tv/99999"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("not valid json"))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let result = client.tv_details(99999, "ja-JP").await;

        // Assert
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("failed to decode JSON"),
            "expected 'failed to decode JSON' in: {err}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_genre_movie_list_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body = include_str!("../../../../fixtures/tmdb/genre_tv_list.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/3/genre/movie/list"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let response = client.genre_movie_list("ja").await.unwrap();

        // Assert
        assert!(!response.genres.is_empty());
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_rate_limiter_enforces_interval() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body = include_str!("../../../../fixtures/tmdb/search_multi_empty.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/3/search/multi"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .expect(2)
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/3/", mock_server.uri());
        let client = TmdbClient::builder()
            .base_url(base_url.parse().unwrap())
            .api_token("test-token")
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(100))
            .build()
            .unwrap();

        let params = SearchMultiParams::new("test");

        // Act
        let start = std::time::Instant::now();
        client.search_multi(&params).await.unwrap();
        client.search_multi(&params).await.unwrap();
        let elapsed = start.elapsed();

        // Assert: at least 100ms interval between two requests
        assert!(elapsed >= Duration::from_millis(100));
    }
}
