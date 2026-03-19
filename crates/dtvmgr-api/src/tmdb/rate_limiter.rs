//! TMDB API rate limiter.

use std::time::Duration;

use crate::rate_limiter::SimpleRateLimiter;

/// Default minimum interval between requests (~40 req/s).
const DEFAULT_MIN_INTERVAL: Duration = Duration::from_millis(25);

/// Creates a TMDB rate limiter with the default interval.
pub const fn default_limiter() -> SimpleRateLimiter {
    SimpleRateLimiter::new(DEFAULT_MIN_INTERVAL, "tmdb")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_default_interval_is_25ms() {
        // Arrange & Act & Assert
        assert_eq!(DEFAULT_MIN_INTERVAL, Duration::from_millis(25));
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
