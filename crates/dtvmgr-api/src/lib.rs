//! API client library for dtvmgr.
//!
//! Provides clients for the Syoboi Calendar API, the TMDB API,
//! and the `EPGStation` API.

/// `EPGStation` API client.
pub mod epgstation;

/// OTel metrics instruments for API clients.
#[cfg(feature = "otel")]
mod metrics;

/// Simple single-tier rate limiter shared across API clients.
mod rate_limiter;

/// Syoboi Calendar API client.
pub mod syoboi;

/// TMDB API client.
pub mod tmdb;

/// Classifies a `reqwest::Error` into a human-readable error kind label.
pub(crate) fn classify_reqwest_error(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
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
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::needless_borrows_for_generic_args)]

    use super::*;

    #[tokio::test]
    async fn test_classify_connect_error() {
        // Arrange: trigger a connection error by connecting to a closed port
        let client = reqwest::Client::builder().build().unwrap();
        let err = client.get("http://127.0.0.1:1").send().await.unwrap_err();

        // Act
        let label = classify_reqwest_error(&err);

        // Assert
        assert_eq!(label, "connection error");
    }

    #[tokio::test]
    async fn test_classify_timeout_error() {
        // Arrange: trigger a timeout with a very short timeout
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_nanos(1))
            .build()
            .unwrap();
        // Use a non-routable address to ensure timeout
        let err = client.get("http://192.0.2.1:1").send().await.unwrap_err();

        // Act
        let label = classify_reqwest_error(&err);

        // Assert: may be timeout or connection error depending on OS speed
        assert!(
            label == "timeout" || label == "connection error",
            "expected timeout or connection error, got: {label}"
        );
    }

    #[tokio::test]
    async fn test_classify_redirect_error() {
        // Arrange: trigger redirect error by disabling redirects
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::any())
            .respond_with(
                wiremock::ResponseTemplate::new(301).insert_header("Location", &server.uri()),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(0))
            .build()
            .unwrap();
        let err = client.get(&server.uri()).send().await.unwrap_err();

        // Act
        let label = classify_reqwest_error(&err);

        // Assert
        assert_eq!(label, "too many redirects");
    }

    #[tokio::test]
    async fn test_classify_decode_error() {
        // Arrange: trigger decode error by expecting JSON from invalid body
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::any())
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let err = reqwest::Client::new()
            .get(&server.uri())
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap_err();

        // Act
        let label = classify_reqwest_error(&err);

        // Assert
        assert_eq!(label, "decode error");
    }
}
