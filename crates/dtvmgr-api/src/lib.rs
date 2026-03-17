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
