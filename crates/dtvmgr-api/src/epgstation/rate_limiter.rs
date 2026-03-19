//! `EPGStation` API rate limiter.

use std::time::Duration;

use crate::rate_limiter::SimpleRateLimiter;

/// Default minimum interval between requests (50ms, ~20 req/s).
const DEFAULT_MIN_INTERVAL: Duration = Duration::from_millis(50);

/// Creates an `EPGStation` rate limiter with the default interval.
pub const fn default_limiter() -> SimpleRateLimiter {
    SimpleRateLimiter::new(DEFAULT_MIN_INTERVAL, "epgstation")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_default_interval_is_50ms() {
        // Arrange & Act
        // Verify the constant matches the expected value.
        // The limiter behavior is tested in crate::rate_limiter::tests.

        // Assert
        assert_eq!(DEFAULT_MIN_INTERVAL, Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_default_limiter_works() {
        // Arrange
        let mut limiter = default_limiter();

        // Act
        limiter.wait().await;

        // Assert — no panic, limiter functions correctly
    }
}
