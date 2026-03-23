//! `SyoboiClient` - Syoboi Calendar API client implementation.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Client;
use tokio::sync::Mutex;
use tracing::instrument;
use url::Url;

use super::api::LocalSyoboiApi;
use super::params::ProgLookupParams;
use super::rate_limiter::SyoboiRateLimiter;
use super::types::{SyoboiChannel, SyoboiChannelGroup, SyoboiProgram, SyoboiTitle};
use super::xml::{
    ApiResult, ChGroupLookupResponse, ChLookupResponse, ProgLookupResponse, TitleLookupResponse,
};

/// Base URL for the Syoboi Calendar website.
pub const SYOBOI_BASE_URL: &str = "https://cal.syoboi.jp";

/// Default base URL.
const DEFAULT_BASE_URL: &str = concat!("https://cal.syoboi.jp", "/db.php");

/// Maximum number of retries for rate-limited (429) responses.
const MAX_RETRIES: u32 = 3;

/// Maximum number of retries for transient network errors (e.g. keep-alive race).
const MAX_NETWORK_RETRIES: u32 = 1;

/// Delay between retries.
const RETRY_DELAY: Duration = Duration::from_secs(2);

/// Syoboi Calendar API client.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct SyoboiClient {
    /// HTTP client (reqwest, gzip enabled).
    http_client: Client,
    /// Base URL.
    base_url: Url,
    /// Rate limiter.
    rate_limiter: Arc<Mutex<SyoboiRateLimiter>>,
}

/// Builder for `SyoboiClient`.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct SyoboiClientBuilder {
    base_url: Option<Url>,
    user_agent: Option<String>,
    min_interval: Option<Duration>,
    hourly_limit: Option<u32>,
    daily_limit: Option<u32>,
}

impl SyoboiClientBuilder {
    /// Creates a new builder.
    const fn new() -> Self {
        Self {
            base_url: None,
            user_agent: None,
            min_interval: None,
            hourly_limit: None,
            daily_limit: None,
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

    /// Sets the minimum request interval (default: 1s).
    #[must_use]
    pub const fn min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = Some(interval);
        self
    }

    /// Sets the hourly request limit (default: 500).
    #[must_use]
    pub const fn hourly_limit(mut self, limit: u32) -> Self {
        self.hourly_limit = Some(limit);
        self
    }

    /// Sets the daily request limit (default: 10,000).
    #[must_use]
    pub const fn daily_limit(mut self, limit: u32) -> Self {
        self.daily_limit = Some(limit);
        self
    }

    /// Builds the client.
    ///
    /// # Errors
    ///
    /// - `user_agent` is not set.
    /// - `reqwest::Client` build fails.
    pub fn build(self) -> Result<SyoboiClient> {
        let user_agent = self.user_agent.context("user_agent is required")?;

        let base_url = if let Some(url) = self.base_url {
            url
        } else {
            let result = Url::parse(DEFAULT_BASE_URL);
            result.context("invalid default base URL")?
        };

        let min_interval = self.min_interval.unwrap_or(Duration::from_secs(1));
        let hourly_limit = self.hourly_limit.unwrap_or(500);
        let daily_limit = self.daily_limit.unwrap_or(10_000);

        let http_client = Client::builder()
            .user_agent(&user_agent)
            .gzip(true)
            .build()
            .context("failed to build HTTP client")?;

        let rate_limiter = Arc::new(Mutex::new(SyoboiRateLimiter::new(
            min_interval,
            usize::try_from(hourly_limit).context("failed to convert hourly_limit")?,
            usize::try_from(daily_limit).context("failed to convert daily_limit")?,
        )));

        Ok(SyoboiClient {
            http_client,
            base_url,
            rate_limiter,
        })
    }
}

impl SyoboiClient {
    /// Creates a new builder.
    #[must_use]
    pub const fn builder() -> SyoboiClientBuilder {
        SyoboiClientBuilder::new()
    }

    /// Checks API result code. Returns an error if code is not 200.
    fn check_api_result(result: Option<&ApiResult>, command: &str) -> Result<()> {
        if let Some(r) = result
            && r.code != 200
        {
            bail!(
                "{} API error: code={}, message={:?}",
                command,
                r.code,
                r.message
            );
        }
        Ok(())
    }

    /// Builds an XML decode error with a preview of the response body.
    fn xml_decode_error(command: &str, xml: &str) -> String {
        let preview_len = xml.len().min(500);
        format!(
            "{} XML decoding failed (len={}): {}",
            command,
            xml.len(),
            &xml[..preview_len]
        )
    }

    /// Parses a `TitleLookup` XML response.
    pub(crate) fn parse_title_response(xml: &str) -> Result<Vec<SyoboiTitle>> {
        let raw_result: std::result::Result<TitleLookupResponse, _> = quick_xml::de::from_str(xml);
        let response = raw_result.with_context(|| Self::xml_decode_error("TitleLookup", xml))?;
        Self::check_api_result(response.result.as_ref(), "TitleLookup")?;
        Ok(response
            .title_items
            .map_or_else(Vec::new, |items| items.items))
    }

    /// Parses a `ProgLookup` XML response.
    pub(crate) fn parse_prog_response(xml: &str) -> Result<Vec<SyoboiProgram>> {
        let raw_result: std::result::Result<ProgLookupResponse, _> = quick_xml::de::from_str(xml);
        let response = raw_result.with_context(|| Self::xml_decode_error("ProgLookup", xml))?;
        Self::check_api_result(response.result.as_ref(), "ProgLookup")?;
        Ok(response
            .prog_items
            .map_or_else(Vec::new, |items| items.items))
    }

    /// Parses a `ChLookup` XML response.
    pub(crate) fn parse_ch_response(xml: &str) -> Result<Vec<SyoboiChannel>> {
        let raw_result: std::result::Result<ChLookupResponse, _> = quick_xml::de::from_str(xml);
        let response = raw_result.with_context(|| Self::xml_decode_error("ChLookup", xml))?;
        Self::check_api_result(response.result.as_ref(), "ChLookup")?;
        Ok(response.ch_items.map_or_else(Vec::new, |items| items.items))
    }

    /// Parses a `ChGroupLookup` XML response.
    pub(crate) fn parse_ch_group_response(xml: &str) -> Result<Vec<SyoboiChannelGroup>> {
        let raw_result: std::result::Result<ChGroupLookupResponse, _> =
            quick_xml::de::from_str(xml);
        let response = raw_result.with_context(|| Self::xml_decode_error("ChGroupLookup", xml))?;
        Self::check_api_result(response.result.as_ref(), "ChGroupLookup")?;
        Ok(response
            .ch_group_items
            .map_or_else(Vec::new, |items| items.items))
    }
}

impl SyoboiClient {
    /// Sends a GET request with retry logic.
    ///
    /// Retries up to `MAX_RETRIES` times on failure, waiting the rate limiter
    /// interval before each attempt. Logs warnings on each retry.
    /// Returns the HTTP status code alongside the parsed result.
    #[instrument(skip_all, fields(
        otel.kind = "Client",
        http.request.method = "GET",
        http.command = command,
        url.full = tracing::field::Empty,
        http.response.status_code = tracing::field::Empty,
        http.response.body = tracing::field::Empty,
    ), err(level = "warn"))]
    async fn request_with_retry<T, F>(
        &self,
        command: &str,
        build_request: impl Fn() -> reqwest::RequestBuilder,
        parse: F,
    ) -> Result<(u16, T)>
    where
        F: Fn(&str) -> Result<T>,
    {
        #[cfg(feature = "otel")]
        let request_start = std::time::Instant::now();
        let mut network_retries = 0u32;
        let mut rate_limit_retries = 0u32;

        loop {
            self.rate_limiter.lock().await.wait().await;

            let response = match build_request().send().await {
                Ok(r) => r,
                Err(e) if !e.is_timeout() && network_retries < MAX_NETWORK_RETRIES => {
                    network_retries = network_retries.saturating_add(1);
                    tracing::debug!(
                        retry = network_retries,
                        error = %e,
                        "transient network error, retrying"
                    );
                    continue;
                }
                Err(e) => {
                    let kind = crate::classify_reqwest_error(&e);
                    bail!("{kind}: {command}: {e:#}");
                }
            };

            let span = tracing::Span::current();
            span.record("url.full", tracing::field::display(response.url()));
            let status = response.status();
            span.record("http.response.status_code", i64::from(status.as_u16()));
            let headers = response.headers().clone();
            tracing::trace!(
                %command,
                %status,
                ?headers,
                "Response headers"
            );

            // Cloudflare rate-limit: respect Retry-After header.
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                #[cfg(feature = "otel")]
                crate::metrics::record_rate_limit_hit("syoboi");

                rate_limit_retries = rate_limit_retries.saturating_add(1);
                if rate_limit_retries > MAX_RETRIES {
                    bail!("Syoboi API rate limited after {MAX_RETRIES} retries: {command}");
                }

                let retry_after = headers
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map_or(RETRY_DELAY, |secs| {
                        Duration::from_secs(secs.saturating_add(1))
                    });

                tracing::warn!(
                    %command,
                    retry = rate_limit_retries,
                    max_retries = MAX_RETRIES,
                    retry_after_secs = retry_after.as_secs(),
                    "Rate limited, waiting before retry"
                );
                tokio::time::sleep(retry_after).await;
                continue;
            }

            let xml = response
                .text()
                .await
                .with_context(|| format!("failed to read {command} response body"))?;

            span.record("http.response.body", xml.as_str());
            tracing::debug!(%command, body_len = xml.len(), "Response body received");

            let result =
                parse(&xml).with_context(|| format!("failed to parse {command} response"))?;

            #[cfg(feature = "otel")]
            crate::metrics::record_request_duration("syoboi", "GET", request_start);

            return Ok((status.as_u16(), result));
        }
    }
}

impl SyoboiClient {
    /// Looks up titles, returning HTTP status code alongside results.
    ///
    /// Use this when the caller needs the HTTP status code (e.g. for
    /// chunk-level logging). The trait method `lookup_titles` discards
    /// the status code.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or XML parsing fails.
    pub async fn lookup_titles_with_status(
        &self,
        tids: &[u32],
        fields: Option<&[&str]>,
    ) -> Result<(u16, Vec<SyoboiTitle>)> {
        let tid_str = tids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");

        let fields_str = fields.map(|f| f.join(","));

        self.request_with_retry(
            "TitleLookup",
            || {
                let mut req = self
                    .http_client
                    .get(self.base_url.clone())
                    .query(&[("Command", "TitleLookup"), ("TID", &*tid_str)]);
                if let Some(ref f) = fields_str {
                    req = req.query(&[("Fields", f.as_str())]);
                }
                req
            },
            Self::parse_title_response,
        )
        .await
    }
}

impl LocalSyoboiApi for SyoboiClient {
    #[instrument(skip_all, fields(otel.kind = "Client"), err(level = "error"))]
    async fn lookup_titles(
        &self,
        tids: &[u32],
        fields: Option<&[&str]>,
    ) -> Result<Vec<SyoboiTitle>> {
        self.lookup_titles_with_status(tids, fields)
            .await
            .map(|(_, titles)| titles)
    }

    #[instrument(skip_all, fields(otel.kind = "Client"), err(level = "error"))]
    async fn lookup_programs(&self, params: &ProgLookupParams) -> Result<Vec<SyoboiProgram>> {
        let query = Self::build_prog_query(params);

        self.request_with_retry(
            "ProgLookup",
            || self.http_client.get(self.base_url.clone()).query(&query),
            Self::parse_prog_response,
        )
        .await
        .map(|(_, data)| data)
    }

    #[instrument(skip_all, fields(otel.kind = "Client"), err(level = "error"))]
    async fn lookup_channels(&self, ch_ids: Option<&[u32]>) -> Result<Vec<SyoboiChannel>> {
        let mut query: Vec<(&str, String)> = vec![("Command", String::from("ChLookup"))];
        if let Some(ch_ids) = ch_ids {
            let ch_id_str = ch_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            query.push(("ChID", ch_id_str));
        }

        self.request_with_retry(
            "ChLookup",
            || self.http_client.get(self.base_url.clone()).query(&query),
            Self::parse_ch_response,
        )
        .await
        .map(|(_, data)| data)
    }

    #[instrument(skip_all, fields(otel.kind = "Client"), err(level = "error"))]
    async fn lookup_channel_groups(
        &self,
        ch_gids: Option<&[u32]>,
    ) -> Result<Vec<SyoboiChannelGroup>> {
        let mut query: Vec<(&str, String)> = vec![("Command", String::from("ChGroupLookup"))];
        let ch_gid_str = ch_gids.map_or_else(
            || String::from("*"),
            |gids| {
                gids.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            },
        );
        query.push(("ChGID", ch_gid_str));

        self.request_with_retry(
            "ChGroupLookup",
            || self.http_client.get(self.base_url.clone()).query(&query),
            Self::parse_ch_group_response,
        )
        .await
        .map(|(_, data)| data)
    }
}

impl SyoboiClient {
    /// Builds query parameters for `ProgLookup`.
    fn build_prog_query(params: &ProgLookupParams) -> Vec<(&'static str, String)> {
        let mut query: Vec<(&str, String)> = vec![("Command", String::from("ProgLookup"))];

        if let Some(ref tids) = params.tids {
            let tid_str = tids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            query.push(("TID", tid_str));
        }

        if let Some(ref ch_ids) = params.ch_ids {
            let ch_id_str = ch_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            query.push(("ChID", ch_id_str));
        }

        if let Some(ref range) = params.range {
            query.push(("Range", range.to_syoboi_format()));
        }

        if let Some(ref st_time) = params.st_time {
            query.push(("StTime", st_time.clone()));
        }

        if let Some(ref last_update) = params.last_update {
            query.push(("LastUpdate", last_update.clone()));
        }

        if params.join_sub_titles {
            query.push(("JOIN", String::from("SubTitles")));
        }

        if let Some(ref fields) = params.fields {
            query.push(("Fields", fields.join(",")));
        }

        query
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]

    use chrono::NaiveDate;

    use super::*;

    #[test]
    fn test_builder_requires_user_agent() {
        // Arrange & Act
        let result = SyoboiClient::builder().build();

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
    fn test_builder_with_user_agent_succeeds() {
        // Arrange & Act
        let result = SyoboiClient::builder().user_agent("test/0.0.0").build();

        // Assert
        assert!(result.is_ok());
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_builder_with_custom_base_url() {
        // Arrange
        let custom_url = Url::parse("http://localhost:8080/db.php").unwrap();

        // Act
        let client = SyoboiClient::builder()
            .base_url(custom_url.clone())
            .user_agent("test/0.0.0")
            .build()
            .unwrap();

        // Assert
        assert_eq!(client.base_url, custom_url);
    }

    #[test]
    fn test_parse_title_response() {
        // Arrange
        let xml = include_str!("../../../../fixtures/syoboi/title_lookup_6309.xml");

        // Act
        let titles = SyoboiClient::parse_title_response(xml).unwrap();

        // Assert
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0].tid, 6309);
        assert_eq!(titles[0].title, "SPY×FAMILY");
        assert_eq!(titles[0].title_en.as_deref(), Some("SPY FAMILY"));
        assert_eq!(titles[0].first_year, Some(2022));
        assert_eq!(titles[0].first_month, Some(4));
        assert!(titles[0].sub_titles.as_ref().unwrap().contains("*01*"));
        // Empty elements should be deserialized as None
        assert_eq!(titles[0].short_title, None);
        assert_eq!(titles[0].keywords, None);
    }

    #[test]
    fn test_parse_prog_response() {
        // Arrange
        let xml = include_str!("../../../../fixtures/syoboi/prog_lookup_6309.xml");

        // Act
        let programs = SyoboiClient::parse_prog_response(xml).unwrap();

        // Assert
        assert_eq!(programs.len(), 3);
        let first = &programs[0];
        assert_eq!(first.pid, 574_823);
        assert_eq!(first.tid, 6309);
        assert_eq!(first.ch_id, 7);
        assert_eq!(first.count, Some(1));
        assert_eq!(
            first.st_sub_title.as_deref(),
            Some("オペレーション〈梟(ストリクス)〉")
        );
        // Empty elements should be deserialized as None
        assert_eq!(first.sub_title, None);
        assert_eq!(first.prog_comment, None);
    }

    #[test]
    fn test_parse_ch_response() {
        // Arrange
        let xml = include_str!("../../../../fixtures/syoboi/ch_lookup_all.xml");

        // Act
        let channels = SyoboiClient::parse_ch_response(xml).unwrap();

        // Assert
        assert_eq!(channels.len(), 3);
        assert_eq!(channels[0].ch_id, 1);
        assert_eq!(channels[0].ch_name, "NHK総合");
        assert_eq!(channels[0].ch_gid, Some(11));
        // Extra fields
        assert_eq!(channels[0].ch_iepg_name.as_deref(), Some("ＮＨＫ総合"));
        assert_eq!(channels[0].ch_number, Some(1));
    }

    #[test]
    fn test_parse_empty_response() {
        // Arrange
        let xml = include_str!("../../../../fixtures/syoboi/empty_response.xml");

        // Act
        let titles = SyoboiClient::parse_title_response(xml).unwrap();

        // Assert
        assert!(titles.is_empty());
    }

    #[test]
    fn test_parse_title_response_without_result() {
        // Arrange: API sometimes omits <Result> element
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<TitleLookupResponse>
    <TitleItems>
        <TitleItem id="100">
            <TID>100</TID>
            <LastUpdate>2024-01-01 00:00:00</LastUpdate>
            <Title>No Result Element</Title>
            <ShortTitle></ShortTitle>
            <TitleYomi></TitleYomi>
            <TitleEN></TitleEN>
            <Comment></Comment>
            <Cat>1</Cat>
            <TitleFlag>0</TitleFlag>
            <FirstYear>2024</FirstYear>
            <FirstMonth>1</FirstMonth>
            <FirstEndYear></FirstEndYear>
            <FirstEndMonth></FirstEndMonth>
            <FirstCh></FirstCh>
            <Keywords></Keywords>
            <UserPoint></UserPoint>
            <UserPointRank></UserPointRank>
            <SubTitles></SubTitles>
        </TitleItem>
    </TitleItems>
</TitleLookupResponse>"#;

        // Act
        let titles = SyoboiClient::parse_title_response(xml).unwrap();

        // Assert
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0].title, "No Result Element");
    }

    #[test]
    fn test_parse_empty_sub_title_as_none() {
        // Arrange
        let xml = r#"
        <ProgLookupResponse>
            <ProgItems>
                <ProgItem id="574823">
                    <PID>574823</PID>
                    <TID>6309</TID>
                    <StTime>2022-04-09 23:00:00</StTime>
                    <EdTime>2022-04-09 23:30:00</EdTime>
                    <Count>1</Count>
                    <SubTitle></SubTitle>
                    <ChID>7</ChID>
                    <STSubTitle>オペレーション〈梟(ストリクス)〉</STSubTitle>
                </ProgItem>
            </ProgItems>
        </ProgLookupResponse>
        "#;

        // Act
        let programs = SyoboiClient::parse_prog_response(xml).unwrap();

        // Assert
        assert_eq!(programs[0].sub_title, None);
        assert_eq!(
            programs[0].st_sub_title.as_deref(),
            Some("オペレーション〈梟(ストリクス)〉")
        );
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_title_lookup_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let xml_body = include_str!("../../../../fixtures/syoboi/title_lookup_6309.xml");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/db.php"))
            .and(wiremock::matchers::query_param("Command", "TitleLookup"))
            .and(wiremock::matchers::query_param("TID", "6309"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(xml_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/db.php", mock_server.uri());
        let client = SyoboiClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let titles = client.lookup_titles(&[6309], None).await.unwrap();

        // Assert
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0].title, "SPY×FAMILY");
        assert_eq!(titles[0].tid, 6309);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_prog_lookup_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let xml_body = include_str!("../../../../fixtures/syoboi/prog_lookup_6309.xml");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/db.php"))
            .and(wiremock::matchers::query_param("Command", "ProgLookup"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(xml_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/db.php", mock_server.uri());
        let client = SyoboiClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        let params = super::super::ProgLookupParams {
            tids: Some(vec![6309]),
            ..Default::default()
        };

        // Act
        let programs = client.lookup_programs(&params).await.unwrap();

        // Assert
        assert_eq!(programs.len(), 3);
        assert_eq!(programs[0].tid, 6309);
        assert_eq!(programs[0].ch_id, 7);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_ch_lookup_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let xml_body = include_str!("../../../../fixtures/syoboi/ch_lookup_all.xml");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/db.php"))
            .and(wiremock::matchers::query_param("Command", "ChLookup"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(xml_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/db.php", mock_server.uri());
        let client = SyoboiClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let channels = client.lookup_channels(None).await.unwrap();

        // Assert
        assert_eq!(channels.len(), 3);
        assert_eq!(channels[0].ch_name, "NHK総合");
    }

    #[test]
    fn test_parse_ch_group_response() {
        // Arrange
        let xml = include_str!("../../../../fixtures/syoboi/ch_group_lookup_all.xml");

        // Act
        let groups = SyoboiClient::parse_ch_group_response(xml).unwrap();

        // Assert
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].ch_gid, 1);
        assert_eq!(groups[0].ch_group_name, "テレビ 関東");
        assert_eq!(groups[0].ch_group_order, 1200);
        assert_eq!(groups[1].ch_gid, 2);
        assert_eq!(groups[1].ch_group_name, "BSデジタル");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_ch_group_lookup_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let xml_body = include_str!("../../../../fixtures/syoboi/ch_group_lookup_all.xml");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/db.php"))
            .and(wiremock::matchers::query_param("Command", "ChGroupLookup"))
            .and(wiremock::matchers::query_param("ChGID", "*"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(xml_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/db.php", mock_server.uri());
        let client = SyoboiClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let groups = client.lookup_channel_groups(None).await.unwrap();

        // Assert
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].ch_group_name, "テレビ 関東");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_rate_limiter_enforces_interval() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_string(
                    "<TitleLookupResponse><Result><Code>200</Code></Result><TitleItems></TitleItems></TitleLookupResponse>",
                ),
            )
            .expect(2)
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/db.php", mock_server.uri());
        let client = SyoboiClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(100))
            .build()
            .unwrap();

        // Act
        let start = std::time::Instant::now();
        client.lookup_titles(&[1], None).await.unwrap();
        client.lookup_titles(&[2], None).await.unwrap();
        let elapsed = start.elapsed();

        // Assert: at least 100ms interval between two requests
        assert!(elapsed >= Duration::from_millis(100));
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_user_agent_is_sent() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::header("User-Agent", "recmgr/0.1.0"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_string(
                    "<TitleLookupResponse><Result><Code>200</Code></Result><TitleItems></TitleItems></TitleLookupResponse>",
                ),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/db.php", mock_server.uri());
        let client = SyoboiClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("recmgr/0.1.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act & Assert (mock expect(1) verifies User-Agent header)
        client.lookup_titles(&[1], None).await.unwrap();
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_title_lookup_with_fields() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let xml_body = include_str!("../../../../fixtures/syoboi/title_lookup_6309.xml");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/db.php"))
            .and(wiremock::matchers::query_param("Command", "TitleLookup"))
            .and(wiremock::matchers::query_param("TID", "6309"))
            .and(wiremock::matchers::query_param("Fields", "TID,Title,Cat"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(xml_body))
            .expect(1)
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/db.php", mock_server.uri());
        let client = SyoboiClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let titles = client
            .lookup_titles(&[6309], Some(&["TID", "Title", "Cat"]))
            .await
            .unwrap();

        // Assert
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0].tid, 6309);
    }

    #[test]
    fn test_check_api_result_error_code() {
        // Arrange
        let result = ApiResult {
            code: 500,
            message: Some(String::from("Internal Server Error")),
        };

        // Act
        let err = SyoboiClient::check_api_result(Some(&result), "TestCommand");

        // Assert
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("TestCommand API error"));
        assert!(msg.contains("code=500"));
    }

    #[test]
    fn test_check_api_result_success() {
        // Arrange
        let result = ApiResult {
            code: 200,
            message: None,
        };

        // Act
        let res = SyoboiClient::check_api_result(Some(&result), "TestCommand");

        // Assert
        assert!(res.is_ok());
    }

    #[test]
    fn test_check_api_result_none() {
        // Arrange & Act
        let res = SyoboiClient::check_api_result(None, "TestCommand");

        // Assert: None result is ok (API sometimes omits Result element)
        assert!(res.is_ok());
    }

    #[test]
    fn test_xml_decode_error_short_body() {
        // Arrange
        let xml = "<short>body</short>";

        // Act
        let msg = SyoboiClient::xml_decode_error("TestCommand", xml);

        // Assert
        assert!(msg.contains("TestCommand XML decoding failed"));
        assert!(msg.contains("<short>body</short>"));
    }

    #[test]
    fn test_xml_decode_error_truncates_long_body() {
        // Arrange: body longer than 500 chars
        let xml = "x".repeat(1000);

        // Act
        let msg = SyoboiClient::xml_decode_error("TestCommand", &xml);

        // Assert: preview is truncated to 500 chars
        assert!(msg.contains("len=1000"));
        assert!(msg.len() < 600); // header + 500 chars
    }

    #[test]
    fn test_parse_title_response_invalid_xml() {
        // Arrange
        let xml = "not valid xml at all";

        // Act
        let result = SyoboiClient::parse_title_response(xml);

        // Assert
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("TitleLookup XML decoding failed"),
            "expected 'TitleLookup XML decoding failed' in: {err}"
        );
    }

    #[test]
    fn test_parse_prog_response_invalid_xml() {
        // Arrange
        let xml = "invalid xml";

        // Act
        let result = SyoboiClient::parse_prog_response(xml);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ch_response_invalid_xml() {
        // Arrange
        let xml = "invalid xml";

        // Act
        let result = SyoboiClient::parse_ch_response(xml);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ch_group_response_invalid_xml() {
        // Arrange
        let xml = "invalid xml";

        // Act
        let result = SyoboiClient::parse_ch_group_response(xml);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_title_response_api_error_code() {
        // Arrange: valid XML but API returns error code
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<TitleLookupResponse>
    <Result><Code>400</Code><Message>Bad Request</Message></Result>
    <TitleItems></TitleItems>
</TitleLookupResponse>"#;

        // Act
        let result = SyoboiClient::parse_title_response(xml);

        // Assert
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("TitleLookup API error"));
        assert!(err.contains("code=400"));
    }

    #[test]
    fn test_parse_prog_response_api_error_code() {
        // Arrange
        let xml = r"<ProgLookupResponse>
    <Result><Code>400</Code><Message>Bad Request</Message></Result>
    <ProgItems></ProgItems>
</ProgLookupResponse>";

        // Act
        let result = SyoboiClient::parse_prog_response(xml);

        // Assert
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("ProgLookup API error"));
    }

    #[test]
    fn test_build_prog_query_all_params() {
        // Arrange
        let params = ProgLookupParams {
            tids: Some(vec![100, 200]),
            ch_ids: Some(vec![1, 2]),
            range: Some(super::super::params::TimeRange::new(
                NaiveDate::from_ymd_opt(2024, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
                NaiveDate::from_ymd_opt(2024, 2, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            )),
            st_time: Some(String::from("2024-01-01")),
            last_update: Some(String::from("2024-01-01")),
            join_sub_titles: true,
            fields: Some(vec![String::from("PID"), String::from("TID")]),
        };

        // Act
        let query = SyoboiClient::build_prog_query(&params);

        // Assert
        let keys: Vec<&str> = query.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"Command"));
        assert!(keys.contains(&"TID"));
        assert!(keys.contains(&"ChID"));
        assert!(keys.contains(&"Range"));
        assert!(keys.contains(&"StTime"));
        assert!(keys.contains(&"LastUpdate"));
        assert!(keys.contains(&"JOIN"));
        assert!(keys.contains(&"Fields"));
    }

    #[test]
    fn test_build_prog_query_minimal() {
        // Arrange
        let params = ProgLookupParams::default();

        // Act
        let query = SyoboiClient::build_prog_query(&params);

        // Assert: Command + JOIN (join_sub_titles defaults to true)
        assert_eq!(query.len(), 2);
        assert_eq!(query[0].0, "Command");
        assert_eq!(query[0].1, "ProgLookup");
        assert_eq!(query[1].0, "JOIN");
        assert_eq!(query[1].1, "SubTitles");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_builder_with_custom_limits() {
        // Arrange & Act
        let client = SyoboiClient::builder()
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(50))
            .hourly_limit(100)
            .daily_limit(1000)
            .build()
            .unwrap();

        // Assert: client built successfully with custom limits
        assert_eq!(client.base_url.as_str(), "https://cal.syoboi.jp/db.php");
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_ch_lookup_with_specific_ids() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let xml_body = include_str!("../../../../fixtures/syoboi/ch_lookup_all.xml");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/db.php"))
            .and(wiremock::matchers::query_param("Command", "ChLookup"))
            .and(wiremock::matchers::query_param("ChID", "1,2"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(xml_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/db.php", mock_server.uri());
        let client = SyoboiClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let channels = client.lookup_channels(Some(&[1, 2])).await.unwrap();

        // Assert
        assert_eq!(channels.len(), 3);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_ch_group_lookup_with_specific_gids() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let xml_body = include_str!("../../../../fixtures/syoboi/ch_group_lookup_all.xml");

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/db.php"))
            .and(wiremock::matchers::query_param("Command", "ChGroupLookup"))
            .and(wiremock::matchers::query_param("ChGID", "1,2"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(xml_body))
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/db.php", mock_server.uri());
        let client = SyoboiClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act
        let groups = client.lookup_channel_groups(Some(&[1, 2])).await.unwrap();

        // Assert
        assert_eq!(groups.len(), 3);
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test]
    async fn test_title_lookup_without_fields_omits_query_param() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let xml_body = "<TitleLookupResponse><Result><Code>200</Code></Result><TitleItems></TitleItems></TitleLookupResponse>";

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/db.php"))
            .and(wiremock::matchers::query_param("Command", "TitleLookup"))
            .and(wiremock::matchers::query_param("TID", "1"))
            .and(wiremock::matchers::query_param_is_missing("Fields"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(xml_body))
            .expect(1)
            .mount(&mock_server)
            .await;

        let base_url = format!("{}/db.php", mock_server.uri());
        let client = SyoboiClient::builder()
            .base_url(base_url.parse().unwrap())
            .user_agent("test/0.0.0")
            .min_interval(Duration::from_millis(0))
            .build()
            .unwrap();

        // Act & Assert (mock expect(1) + query_param_is_missing verifies no Fields param)
        client.lookup_titles(&[1], None).await.unwrap();
    }
}
