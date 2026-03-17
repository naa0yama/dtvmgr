//! `EpgStationClient` - `EPGStation` API client implementation.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Client;
use tokio::sync::Mutex;
use tracing::instrument;
use url::Url;

use super::api::LocalEpgStationApi;
use super::rate_limiter::EpgStationRateLimiter;
use super::types::{
    Channel, EncodeInfoResponse, EncodeRequest, EncodeResponse, EpgConfig, RecordedItem,
    RecordedParams, RecordedResponse,
};

/// Default base URL for local `EPGStation`.
const DEFAULT_BASE_URL: &str = "http://localhost:8888/api/";

/// Maximum number of retries for HTTP 429 responses.
const MAX_RETRIES: u32 = 3;

/// Backoff duration between retries.
const RETRY_BACKOFF: Duration = Duration::from_secs(1);

/// `EPGStation` API client.
#[derive(Debug, Clone)]
#[allow(clippy::module_name_repetitions)]
pub struct EpgStationClient {
    /// HTTP client.
    http_client: Client,
    /// Base URL for API requests.
    base_url: Url,
    /// Rate limiter.
    rate_limiter: Arc<Mutex<EpgStationRateLimiter>>,
}

/// Builder for `EpgStationClient`.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct EpgStationClientBuilder {
    base_url: Option<Url>,
    user_agent: Option<String>,
    min_interval: Option<Duration>,
}

impl EpgStationClientBuilder {
    /// Creates a new builder.
    const fn new() -> Self {
        Self {
            base_url: None,
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

    /// Sets the User-Agent (required).
    #[must_use]
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Sets the minimum request interval (default: 50ms).
    #[must_use]
    pub const fn min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = Some(interval);
        self
    }

    /// Builds the client.
    ///
    /// # Errors
    ///
    /// - `user_agent` is not set.
    /// - `reqwest::Client` build fails.
    pub fn build(self) -> Result<EpgStationClient> {
        let user_agent = self.user_agent.context("user_agent is required")?;

        let base_url = if let Some(url) = self.base_url {
            url
        } else {
            let result = Url::parse(DEFAULT_BASE_URL);
            result.context("invalid default base URL")?
        };

        let rate_limiter = self.min_interval.map_or_else(
            EpgStationRateLimiter::default_interval,
            EpgStationRateLimiter::new,
        );

        let http_client = Client::builder()
            .user_agent(&user_agent)
            .gzip(true)
            .build()
            .context("failed to build HTTP client")?;

        Ok(EpgStationClient {
            http_client,
            base_url,
            rate_limiter: Arc::new(Mutex::new(rate_limiter)),
        })
    }
}

impl EpgStationClient {
    /// Creates a new builder.
    #[must_use]
    pub const fn builder() -> EpgStationClientBuilder {
        EpgStationClientBuilder::new()
    }

    /// Sends a GET request with rate limiting and returns parsed JSON.
    /// Retries up to `MAX_RETRIES` times on HTTP 429.
    #[instrument(skip_all, fields(
        otel.kind = "Client",
        http.method = "GET",
        http.path = path,
        http.url = tracing::field::Empty,
        http.status_code = tracing::field::Empty,
        http.response.body = tracing::field::Empty,
    ), err(level = "error"))]
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        self.rate_limiter.lock().await.wait().await;

        let url = self
            .base_url
            .join(path)
            .with_context(|| format!("failed to join URL path: {path}"))?;

        let mut retries = 0u32;
        loop {
            let request = self
                .http_client
                .get(url.clone())
                .query(query)
                .build()
                .with_context(|| format!("failed to build request: {path}"))?;

            tracing::Span::current().record("http.url", tracing::field::display(request.url()));

            let response = match self.http_client.execute(request).await {
                Ok(resp) => resp,
                Err(e) => {
                    let kind = if e.is_timeout() {
                        "timeout"
                    } else if e.is_connect() {
                        "connection error"
                    } else if e.is_body() {
                        "body error"
                    } else if e.is_decode() {
                        "decode error"
                    } else if e.is_redirect() {
                        "too many redirects"
                    } else {
                        "request error"
                    };
                    if let Some(status) = e.status() {
                        tracing::Span::current()
                            .record("http.status_code", i64::from(status.as_u16()));
                    }
                    bail!("{kind}: {path}: {e:#}");
                }
            };

            let span = tracing::Span::current();
            let status = response.status();
            span.record("http.status_code", i64::from(status.as_u16()));

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                retries = retries.saturating_add(1);
                if retries > MAX_RETRIES {
                    bail!("EPGStation API rate limit exceeded after {MAX_RETRIES} retries: {path}");
                }
                tracing::warn!(
                    retry = retries,
                    max_retries = MAX_RETRIES,
                    "EPGStation API rate limited (429). Retrying..."
                );
                tokio::time::sleep(RETRY_BACKOFF.saturating_mul(retries)).await;
                self.rate_limiter.lock().await.wait().await;
                continue;
            }

            if !status.is_success() {
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| String::from("<failed to read body>"));
                span.record("http.response.body", &body);
                bail!("EPGStation API error (HTTP {status}): {body}");
            }

            let body = response
                .text()
                .await
                .with_context(|| format!("failed to read response body: {path}"))?;
            span.record("http.response.body", body.as_str());
            let raw_result: std::result::Result<T, _> = serde_json::from_str(&body);
            let parsed = raw_result
                .with_context(|| format!("failed to decode JSON response: {path} body={body}"))?;
            return Ok(parsed);
        }
    }

    /// Sends a POST request with JSON body and rate limiting.
    /// Retries up to `MAX_RETRIES` times on HTTP 429.
    #[instrument(skip_all, fields(
        otel.kind = "Client",
        http.method = "POST",
        http.path = path,
        http.url = tracing::field::Empty,
        http.status_code = tracing::field::Empty,
        http.request.body = tracing::field::Empty,
        http.response.body = tracing::field::Empty,
    ), err(level = "error"))]
    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &(impl serde::Serialize + Sync),
    ) -> Result<T> {
        self.rate_limiter.lock().await.wait().await;

        let span = tracing::Span::current();
        if let Ok(request_json) = serde_json::to_string(body) {
            span.record("http.request.body", &request_json);
        }

        let url = self
            .base_url
            .join(path)
            .with_context(|| format!("failed to join URL path: {path}"))?;

        let mut retries = 0u32;
        loop {
            let request = self
                .http_client
                .post(url.clone())
                .json(body)
                .build()
                .with_context(|| format!("failed to build request: {path}"))?;

            span.record("http.url", tracing::field::display(request.url()));

            let response = match self.http_client.execute(request).await {
                Ok(resp) => resp,
                Err(e) => {
                    let kind = if e.is_timeout() {
                        "timeout"
                    } else if e.is_connect() {
                        "connection error"
                    } else if e.is_body() {
                        "body error"
                    } else if e.is_decode() {
                        "decode error"
                    } else if e.is_redirect() {
                        "too many redirects"
                    } else {
                        "request error"
                    };
                    if let Some(status) = e.status() {
                        tracing::Span::current()
                            .record("http.status_code", i64::from(status.as_u16()));
                    }
                    bail!("{kind}: {path}: {e:#}");
                }
            };

            let span = tracing::Span::current();
            let status = response.status();
            span.record("http.status_code", i64::from(status.as_u16()));

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                retries = retries.saturating_add(1);
                if retries > MAX_RETRIES {
                    bail!("EPGStation API rate limit exceeded after {MAX_RETRIES} retries: {path}");
                }
                tracing::warn!(
                    retry = retries,
                    max_retries = MAX_RETRIES,
                    "EPGStation API rate limited (429). Retrying..."
                );
                tokio::time::sleep(RETRY_BACKOFF.saturating_mul(retries)).await;
                self.rate_limiter.lock().await.wait().await;
                continue;
            }

            if !status.is_success() {
                let resp_body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| String::from("<failed to read body>"));
                span.record("http.response.body", &resp_body);
                bail!("EPGStation API error (HTTP {status}): {resp_body}");
            }

            let resp_body = response
                .text()
                .await
                .with_context(|| format!("failed to read response body: {path}"))?;
            span.record("http.response.body", resp_body.as_str());
            let raw_result: std::result::Result<T, _> = serde_json::from_str(&resp_body);
            let parsed = raw_result.with_context(|| {
                format!("failed to decode JSON response: {path} body={resp_body}")
            })?;
            return Ok(parsed);
        }
    }
}

impl EpgStationClient {
    /// Sends a HEAD request and returns whether the response is 200 OK.
    ///
    /// Returns `false` on any non-200 status or network error.
    async fn head_exists(&self, path: &str) -> bool {
        self.rate_limiter.lock().await.wait().await;

        let Ok(url) = self.base_url.join(path) else {
            return false;
        };

        let Ok(request) = self.http_client.head(url).build() else {
            return false;
        };

        tracing::debug!(url = %request.url(), "EPGStation API HEAD request");

        self.http_client
            .execute(request)
            .await
            .is_ok_and(|resp| resp.status() == reqwest::StatusCode::OK)
    }
}

impl LocalEpgStationApi for EpgStationClient {
    #[instrument(skip_all)]
    async fn fetch_recorded(&self, params: &RecordedParams) -> Result<RecordedResponse> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(v) = params.has_original_file {
            query.push(("hasOriginalFile", v.to_string()));
        }
        if let Some(v) = params.limit {
            query.push(("limit", v.to_string()));
        }
        if let Some(v) = params.offset {
            query.push(("offset", v.to_string()));
        }
        if let Some(v) = params.is_reverse {
            query.push(("isReverse", v.to_string()));
        }
        if let Some(v) = params.is_half_width {
            query.push(("isHalfWidth", v.to_string()));
        }
        if let Some(ref v) = params.keyword {
            query.push(("keyword", v.clone()));
        }
        self.get_json("recorded", &query).await
    }

    #[instrument(skip_all)]
    async fn fetch_recorded_by_id(&self, id: u64) -> Result<RecordedItem> {
        self.get_json(
            &format!("recorded/{id}"),
            &[("isHalfWidth", String::from("true"))],
        )
        .await
    }

    #[instrument(skip_all)]
    async fn fetch_channels(&self) -> Result<Vec<Channel>> {
        self.get_json("channels", &[("isHalfWidth", String::from("true"))])
            .await
    }

    #[instrument(skip_all)]
    async fn fetch_config(&self) -> Result<EpgConfig> {
        self.get_json("config", &[]).await
    }

    #[instrument(skip_all)]
    async fn add_encode(&self, body: &EncodeRequest) -> Result<EncodeResponse> {
        self.post_json("encode", body).await
    }

    #[instrument(skip_all)]
    async fn fetch_encode_queue(&self) -> Result<EncodeInfoResponse> {
        self.get_json("encode", &[("isHalfWidth", String::from("true"))])
            .await
    }

    #[instrument(skip_all)]
    async fn check_video_file_exists(&self, video_file_id: u64) -> bool {
        self.head_exists(&format!("videos/{video_file_id}")).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]
    #![allow(clippy::panic)]

    use super::*;

    #[test]
    fn test_builder_requires_user_agent() {
        // Arrange & Act
        let result = EpgStationClient::builder().build();

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
        let result = EpgStationClient::builder().user_agent("test/0.0.0").build();

        // Assert
        assert!(result.is_ok());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_builder_with_custom_base_url() {
        // Arrange
        let custom_url = Url::parse("http://192.168.1.100:8888/api/").unwrap();

        // Act
        let client = EpgStationClient::builder()
            .base_url(custom_url.clone())
            .user_agent("test/0.0.0")
            .build()
            .unwrap();

        // Assert
        assert_eq!(client.base_url, custom_url);
    }

    #[test]
    fn test_parse_recorded_response() {
        // Arrange
        let json = include_str!("../../../../fixtures/epgstation/recorded.json");

        // Act
        let response: RecordedResponse = serde_json::from_str(json).unwrap();

        // Assert
        assert_eq!(response.total, 2);
        assert_eq!(response.records.len(), 2);
        assert_eq!(response.records[0].id, 12345);
        assert_eq!(response.records[0].name, "SPY×FAMILY #01");
        assert_eq!(response.records[0].channel_id, 400_101);
        assert!(!response.records[0].video_files.is_empty());
        assert_eq!(response.records[0].video_files[0].file_type, "ts");
    }

    #[test]
    fn test_parse_channels_response() {
        // Arrange
        let json = include_str!("../../../../fixtures/epgstation/channels.json");

        // Act
        let channels: Vec<Channel> = serde_json::from_str(json).unwrap();

        // Assert
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].id, 400_101);
        assert_eq!(channels[0].name, "NHK総合");
        assert_eq!(channels[0].channel_type, "GR");
    }

    #[test]
    fn test_parse_config_response() {
        // Arrange
        let json = include_str!("../../../../fixtures/epgstation/config.json");

        // Act
        let config: EpgConfig = serde_json::from_str(json).unwrap();

        // Assert
        assert!(!config.encode.is_empty());
        assert_eq!(config.encode[0].name, "H.264");
        assert!(!config.recorded.is_empty());
        assert_eq!(config.recorded[0].name, "recorded");
    }

    #[test]
    fn test_parse_encode_response() {
        // Arrange
        let json = r#"{"encodeId": 42}"#;

        // Act
        let response: EncodeResponse = serde_json::from_str(json).unwrap();

        // Assert
        assert_eq!(response.encode_id, 42);
    }

    #[test]
    fn test_parse_encode_queue_response() {
        // Arrange
        let json = include_str!("../../../../fixtures/epgstation/encode_queue.json");

        // Act
        let response: EncodeInfoResponse = serde_json::from_str(json).unwrap();

        // Assert
        assert_eq!(response.running_items.len(), 1);
        assert_eq!(response.running_items[0].recorded.id, 12345);
        assert_eq!(response.running_items[0].mode, "H.264");
        assert!((response.running_items[0].percent.unwrap() - 45.2).abs() < f64::EPSILON);
        assert_eq!(response.wait_items.len(), 1);
        assert_eq!(response.wait_items[0].recorded.id, 12346);
    }

    #[test]
    fn test_serialize_encode_request() {
        // Arrange
        let request = EncodeRequest {
            recorded_id: 12345,
            source_video_file_id: 67890,
            mode: String::from("H.264"),
            parent_dir: Some(String::from("recorded")),
            directory: Some(String::from("anime")),
            is_save_same_directory: false,
            remove_original: false,
        };

        // Act
        let json = serde_json::to_string(&request).unwrap();

        // Assert
        assert!(json.contains("\"recordedId\":12345"));
        assert!(json.contains("\"sourceVideoFileId\":67890"));
        assert!(json.contains("\"mode\":\"H.264\""));
        assert!(json.contains("\"parentDir\":\"recorded\""));
        assert!(json.contains("\"directory\":\"anime\""));
        assert!(json.contains("\"isSaveSameDirectory\":false"));
        assert!(json.contains("\"removeOriginal\":false"));
    }

    #[test]
    fn test_serialize_encode_request_skips_none() {
        // Arrange
        let request = EncodeRequest {
            recorded_id: 12345,
            source_video_file_id: 67890,
            mode: String::from("H.264"),
            parent_dir: None,
            directory: None,
            is_save_same_directory: true,
            remove_original: false,
        };

        // Act
        let json = serde_json::to_string(&request).unwrap();

        // Assert
        assert!(!json.contains("parentDir"));
        assert!(!json.contains("directory"));
        assert!(json.contains("\"isSaveSameDirectory\":true"));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_fetch_recorded_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body = include_str!("../../../../fixtures/epgstation/recorded.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/recorded"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/api/", mock_server.uri());
        let client = EpgStationClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        let params = RecordedParams {
            has_original_file: Some(true),
            limit: Some(100),
            ..RecordedParams::default()
        };

        // Act
        let response = client.fetch_recorded(&params).await.unwrap();

        // Assert
        assert_eq!(response.total, 2);
        assert_eq!(response.records.len(), 2);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_fetch_channels_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body = include_str!("../../../../fixtures/epgstation/channels.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/channels"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/api/", mock_server.uri());
        let client = EpgStationClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let channels = client.fetch_channels().await.unwrap();

        // Assert
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "NHK総合");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_fetch_config_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body = include_str!("../../../../fixtures/epgstation/config.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/config"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/api/", mock_server.uri());
        let client = EpgStationClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let config = client.fetch_config().await.unwrap();

        // Assert
        assert!(!config.encode.is_empty());
        assert_eq!(config.encode[0].name, "H.264");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_add_encode_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/encode"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_string(r#"{"encodeId": 42}"#),
            )
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/api/", mock_server.uri());
        let client = EpgStationClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        let request = EncodeRequest {
            recorded_id: 12345,
            source_video_file_id: 67890,
            mode: String::from("H.264"),
            parent_dir: Some(String::from("recorded")),
            directory: Some(String::from("anime")),
            is_save_same_directory: false,
            remove_original: false,
        };

        // Act
        let response = client.add_encode(&request).await.unwrap();

        // Assert
        assert_eq!(response.encode_id, 42);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_http_error_returns_error() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/channels"))
            .respond_with(
                wiremock::ResponseTemplate::new(500).set_body_string("Internal Server Error"),
            )
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/api/", mock_server.uri());
        let client = EpgStationClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let result: Result<Vec<Channel>> = client.fetch_channels().await;

        // Assert
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("EPGStation API error")
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_http_429_retries() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/channels"))
            .respond_with(wiremock::ResponseTemplate::new(429).set_body_string("Too Many Requests"))
            .expect(u64::from(MAX_RETRIES) + 1)
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/api/", mock_server.uri());
        let client = EpgStationClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let result: Result<Vec<Channel>> = client.fetch_channels().await;

        // Assert
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rate limit"));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_check_video_file_exists_returns_true() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("HEAD"))
            .and(wiremock::matchers::path("/api/videos/12345"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/api/", mock_server.uri());
        let client = EpgStationClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let exists = client.check_video_file_exists(12345).await;

        // Assert
        assert!(exists);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_check_video_file_exists_returns_false() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("HEAD"))
            .and(wiremock::matchers::path("/api/videos/99999"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/api/", mock_server.uri());
        let client = EpgStationClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let exists = client.check_video_file_exists(99999).await;

        // Assert
        assert!(!exists);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_rate_limiter_enforces_interval() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let json_body = include_str!("../../../../fixtures/epgstation/channels.json");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/channels"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(json_body))
            .expect(2)
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/api/", mock_server.uri());
        let client = EpgStationClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(100))
            .build()
            .unwrap();

        // Act
        let start = std::time::Instant::now();
        let _: Vec<Channel> = client.fetch_channels().await.unwrap();
        let _: Vec<Channel> = client.fetch_channels().await.unwrap();
        let elapsed = start.elapsed();

        // Assert: at least 100ms interval between two requests
        assert!(elapsed >= Duration::from_millis(100));
    }
}
