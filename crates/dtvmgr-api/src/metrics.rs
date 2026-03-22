//! OTel metrics instruments for API clients.

use std::sync::LazyLock;
use std::time::Instant;

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter};

/// Shared meter for dtvmgr-api.
static METER: LazyLock<Meter> = LazyLock::new(|| opentelemetry::global::meter("dtvmgr-api"));

/// Duration of HTTP client requests in seconds.
static HTTP_REQUEST_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    METER
        .f64_histogram("dtvmgr.http.client.request.duration")
        .with_description("Duration of HTTP client requests")
        .with_unit("s")
        .build()
});

/// Number of HTTP request retries.
static HTTP_REQUEST_RETRIES: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER
        .u64_counter("dtvmgr.http.client.request.retries")
        .with_description("Number of HTTP request retries")
        .build()
});

/// Number of HTTP 429 rate limit responses received.
static RATE_LIMIT_HITS: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER
        .u64_counter("dtvmgr.http.client.rate_limit.hits")
        .with_description("Number of HTTP 429 rate limit responses")
        .build()
});

/// Time spent waiting for the rate limiter in seconds.
static RATE_LIMIT_WAIT_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    METER
        .f64_histogram("dtvmgr.http.client.rate_limit.wait_duration")
        .with_description("Time spent waiting for rate limiter")
        .with_unit("s")
        .build()
});

/// Records an HTTP 429 rate limit hit and retry for the given client.
pub fn record_rate_limit_hit(client: &'static str) {
    RATE_LIMIT_HITS.add(1, &[KeyValue::new("client", client)]);
    HTTP_REQUEST_RETRIES.add(
        1,
        &[
            KeyValue::new("client", client),
            KeyValue::new("reason", "rate_limited"),
        ],
    );
}

/// Records the duration of a successful HTTP request.
pub fn record_request_duration(client: &'static str, method: &'static str, start: Instant) {
    HTTP_REQUEST_DURATION.record(
        start.elapsed().as_secs_f64(),
        &[
            KeyValue::new("client", client),
            KeyValue::new("http.request.method", method),
        ],
    );
}

/// Records non-zero wait duration from a rate limiter.
pub fn record_rate_limit_wait(client: &'static str, start: Instant) {
    let waited = start.elapsed();
    if !waited.is_zero() {
        RATE_LIMIT_WAIT_DURATION.record(waited.as_secs_f64(), &[KeyValue::new("client", client)]);
    }
}
