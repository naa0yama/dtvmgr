//! `EPGStation` API rate limiter.

use std::time::{Duration, Instant};

/// Default minimum interval between requests (50ms, ~20 req/s).
const DEFAULT_MIN_INTERVAL: Duration = Duration::from_millis(50);

/// Simple single-tier rate limiter for local `EPGStation` API.
///
/// Ensures a minimum interval between consecutive requests
/// to avoid overwhelming the local server.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct EpgStationRateLimiter {
    /// Minimum interval between requests.
    min_interval: Duration,
    /// Last request timestamp.
    last_request: Option<Instant>,
}

impl EpgStationRateLimiter {
    /// Creates a new rate limiter with the given minimum interval.
    pub(crate) const fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_request: None,
        }
    }

    /// Creates a new rate limiter with the default interval (50ms).
    pub(crate) const fn default_interval() -> Self {
        Self::new(DEFAULT_MIN_INTERVAL)
    }

    /// Waits until the next request is allowed.
    #[allow(clippy::arithmetic_side_effects)]
    pub async fn wait(&mut self) {
        let now = Instant::now();

        if let Some(last) = self.last_request {
            let elapsed = now.duration_since(last);
            if elapsed < self.min_interval {
                tokio::time::sleep(self.min_interval.saturating_sub(elapsed)).await;
            }
        }

        self.last_request = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_first_request_no_wait() {
        // Arrange
        let mut limiter = EpgStationRateLimiter::new(Duration::from_secs(1));

        // Act
        let start = Instant::now();
        limiter.wait().await;
        let elapsed = start.elapsed();

        // Assert
        assert!(elapsed < Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_rate_limiter_min_interval() {
        // Arrange
        let mut limiter = EpgStationRateLimiter::new(Duration::from_millis(50));

        // Act
        let start = Instant::now();
        limiter.wait().await;
        limiter.wait().await;
        let elapsed = start.elapsed();

        // Assert
        assert!(elapsed >= Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_rate_limiter_records_timestamp() {
        // Arrange
        let mut limiter = EpgStationRateLimiter::new(Duration::from_millis(0));

        // Act
        limiter.wait().await;

        // Assert
        assert!(limiter.last_request.is_some());
    }

    #[test]
    fn test_default_interval() {
        // Arrange & Act
        let limiter = EpgStationRateLimiter::default_interval();

        // Assert
        assert_eq!(limiter.min_interval, Duration::from_millis(50));
    }
}
