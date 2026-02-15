//! `SyoboiClient` - Syoboi Calendar API client implementation.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Client;
use tokio::sync::Mutex;
use url::Url;

use super::api::LocalSyoboiApi;
use super::params::ProgLookupParams;
use super::rate_limiter::SyoboiRateLimiter;
use super::types::{SyoboiChannel, SyoboiProgram, SyoboiTitle};
use super::xml::{ChLookupResponse, ProgLookupResponse, TitleLookupResponse};

/// Default base URL.
const DEFAULT_BASE_URL: &str = "https://cal.syoboi.jp/db.php";

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

    /// Parses a `TitleLookup` XML response.
    pub(crate) fn parse_title_response(xml: &str) -> Result<Vec<SyoboiTitle>> {
        let raw_result: std::result::Result<TitleLookupResponse, _> = quick_xml::de::from_str(xml);
        let response = raw_result.context("TitleLookup XML decoding failed")?;

        if response.result.code != 200 {
            bail!(
                "TitleLookup API error: code={}, message={:?}",
                response.result.code,
                response.result.message
            );
        }

        Ok(response.title_items.items)
    }

    /// Parses a `ProgLookup` XML response.
    pub(crate) fn parse_prog_response(xml: &str) -> Result<Vec<SyoboiProgram>> {
        let raw_result: std::result::Result<ProgLookupResponse, _> = quick_xml::de::from_str(xml);
        let response = raw_result.context("ProgLookup XML decoding failed")?;
        Ok(response.prog_items.items)
    }

    /// Parses a `ChLookup` XML response.
    pub(crate) fn parse_ch_response(xml: &str) -> Result<Vec<SyoboiChannel>> {
        let raw_result: std::result::Result<ChLookupResponse, _> = quick_xml::de::from_str(xml);
        let response = raw_result.context("ChLookup XML decoding failed")?;
        Ok(response.ch_items.items)
    }
}

impl LocalSyoboiApi for SyoboiClient {
    async fn lookup_titles(&self, tids: &[u32]) -> Result<Vec<SyoboiTitle>> {
        self.rate_limiter.lock().await.wait().await;

        let tid_str = tids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");

        let result = self
            .http_client
            .get(self.base_url.clone())
            .query(&[("Command", "TitleLookup"), ("TID", &tid_str)])
            .send()
            .await;
        let response = result.context("TitleLookup request failed")?;

        let result = response.text().await;
        let xml = result.context("failed to read TitleLookup response")?;

        Self::parse_title_response(&xml)
    }

    async fn lookup_programs(&self, params: &ProgLookupParams) -> Result<Vec<SyoboiProgram>> {
        self.rate_limiter.lock().await.wait().await;

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

        let request = self
            .http_client
            .get(self.base_url.clone())
            .query(&query)
            .build()
            .context("failed to build ProgLookup request")?;

        tracing::debug!(url = %request.url(), "ProgLookup request URL");

        let result = self.http_client.execute(request).await;
        let response = result.context("ProgLookup request failed")?;

        let result = response.text().await;
        let xml = result.context("failed to read ProgLookup response")?;

        Self::parse_prog_response(&xml)
    }

    async fn lookup_channels(&self, ch_ids: Option<&[u32]>) -> Result<Vec<SyoboiChannel>> {
        self.rate_limiter.lock().await.wait().await;

        let mut query: Vec<(&str, String)> = vec![("Command", String::from("ChLookup"))];

        if let Some(ch_ids) = ch_ids {
            let ch_id_str = ch_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            query.push(("ChID", ch_id_str));
        }

        let result = self
            .http_client
            .get(self.base_url.clone())
            .query(&query)
            .send()
            .await;
        let response = result.context("ChLookup request failed")?;

        let result = response.text().await;
        let xml = result.context("failed to read ChLookup response")?;

        Self::parse_ch_response(&xml)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]

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

    #[test]
    fn test_builder_with_user_agent_succeeds() {
        // Arrange & Act
        let result = SyoboiClient::builder().user_agent("test/0.0.0").build();

        // Assert
        assert!(result.is_ok());
    }

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
        let xml = include_str!("../../../fixtures/syoboi/title_lookup_6309.xml");

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
        let xml = include_str!("../../../fixtures/syoboi/prog_lookup_6309.xml");

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
        let xml = include_str!("../../../fixtures/syoboi/ch_lookup_all.xml");

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
        let xml = include_str!("../../../fixtures/syoboi/empty_response.xml");

        // Act
        let titles = SyoboiClient::parse_title_response(xml).unwrap();

        // Assert
        assert!(titles.is_empty());
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

    #[tokio::test]
    async fn test_title_lookup_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let xml_body = include_str!("../../../fixtures/syoboi/title_lookup_6309.xml");

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
        let titles = client.lookup_titles(&[6309]).await.unwrap();

        // Assert
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0].title, "SPY×FAMILY");
        assert_eq!(titles[0].tid, 6309);
    }

    #[tokio::test]
    async fn test_prog_lookup_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let xml_body = include_str!("../../../fixtures/syoboi/prog_lookup_6309.xml");

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

    #[tokio::test]
    async fn test_ch_lookup_via_http() {
        // Arrange
        let mock_server = wiremock::MockServer::start().await;
        let xml_body = include_str!("../../../fixtures/syoboi/ch_lookup_all.xml");

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
        client.lookup_titles(&[1]).await.unwrap();
        client.lookup_titles(&[2]).await.unwrap();
        let elapsed = start.elapsed();

        // Assert: at least 100ms interval between two requests
        assert!(elapsed >= Duration::from_millis(100));
    }

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
        client.lookup_titles(&[1]).await.unwrap();
    }
}
