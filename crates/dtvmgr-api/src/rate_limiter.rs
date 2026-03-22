//! Simple single-tier rate limiter shared across API clients.

use std::time::{Duration, Instant};

/// Single-tier rate limiter enforcing a minimum interval between requests.
///
/// Used by `EPGStation` and TMDB clients. `SyoboiRateLimiter` uses a separate
/// three-tier limiter due to hourly/daily limits.
#[derive(Debug)]
pub struct SimpleRateLimiter {
    /// Minimum interval between requests.
    min_interval: Duration,
    /// Last request timestamp.
    last_request: Option<Instant>,
    /// Client label for `OTel` metrics (always stored for uniform API).
    #[cfg_attr(not(feature = "otel"), allow(dead_code))]
    client: &'static str,
}

impl SimpleRateLimiter {
    /// Creates a new rate limiter with the given minimum interval.
    pub const fn new(min_interval: Duration, client: &'static str) -> Self {
        Self {
            min_interval,
            last_request: None,
            client,
        }
    }

    /// Waits until the next request is allowed.
    #[allow(clippy::arithmetic_side_effects)]
    pub async fn wait(&mut self) {
        #[cfg(feature = "otel")]
        let wait_start = Instant::now();
        let now = Instant::now();

        if let Some(last) = self.last_request {
            let elapsed = now.duration_since(last);
            if elapsed < self.min_interval {
                tokio::time::sleep(self.min_interval.saturating_sub(elapsed)).await;
            }
        }

        self.last_request = Some(Instant::now());

        #[cfg(feature = "otel")]
        crate::metrics::record_rate_limit_wait(self.client, wait_start);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn test_first_request_no_wait() {
        // Arrange
        let mut limiter = SimpleRateLimiter::new(Duration::from_secs(1), "test");

        // Act
        let start = Instant::now();
        limiter.wait().await;
        let elapsed = start.elapsed();

        // Assert
        assert!(elapsed < Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_min_interval() {
        // Arrange
        let mut limiter = SimpleRateLimiter::new(Duration::from_millis(50), "test");

        // Act
        let start = Instant::now();
        limiter.wait().await;
        limiter.wait().await;
        let elapsed = start.elapsed();

        // Assert
        assert!(elapsed >= Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_records_timestamp() {
        // Arrange
        let mut limiter = SimpleRateLimiter::new(Duration::from_millis(0), "test");

        // Act
        limiter.wait().await;

        // Assert
        assert!(limiter.last_request.is_some());
    }
}
