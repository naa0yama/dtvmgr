//! Syoboi API rate limiter.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Three-tier rate limiter (per-second, hourly, daily).
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct SyoboiRateLimiter {
    /// Minimum interval between requests (default: 1s).
    min_interval: Duration,
    /// Last request timestamp.
    last_request: Option<Instant>,
    /// Hourly request limit (default: 500).
    hourly_limit: usize,
    /// Daily request limit (default: 10,000).
    daily_limit: usize,
    /// Request timestamps within the last hour.
    hourly_window: VecDeque<Instant>,
    /// Request timestamps within the last day.
    daily_window: VecDeque<Instant>,
}

impl SyoboiRateLimiter {
    /// Creates a new rate limiter.
    pub(crate) const fn new(
        min_interval: Duration,
        hourly_limit: usize,
        daily_limit: usize,
    ) -> Self {
        Self {
            min_interval,
            last_request: None,
            hourly_limit,
            daily_limit,
            hourly_window: VecDeque::new(),
            daily_window: VecDeque::new(),
        }
    }

    /// Waits until the next request is allowed.
    ///
    /// Sleeps until all three rate limit tiers are satisfied.
    #[allow(clippy::arithmetic_side_effects)]
    pub async fn wait(&mut self) {
        #[cfg(feature = "otel")]
        let wait_start = Instant::now();
        let now = Instant::now();

        // 1. Purge expired timestamps
        self.cleanup_windows(now);

        // 2. Per-second limit: wait until min_interval has elapsed
        if let Some(last) = self.last_request {
            let elapsed = now.duration_since(last);
            if elapsed < self.min_interval {
                tokio::time::sleep(self.min_interval.saturating_sub(elapsed)).await;
            }
        }

        // 3. Hourly limit
        if self.hourly_window.len() >= self.hourly_limit
            && let Some(&oldest) = self.hourly_window.front()
        {
            let wait_until = oldest + Duration::from_secs(3600);
            let now = Instant::now();
            if now < wait_until {
                tracing::warn!(
                    remaining_secs = (wait_until - now).as_secs(),
                    "Hourly rate limit reached. Waiting..."
                );
                tokio::time::sleep(wait_until - now).await;
            }
        }

        // 4. Daily limit
        if self.daily_window.len() >= self.daily_limit
            && let Some(&oldest) = self.daily_window.front()
        {
            let wait_until = oldest + Duration::from_secs(86400);
            let now = Instant::now();
            if now < wait_until {
                tracing::warn!(
                    remaining_secs = (wait_until - now).as_secs(),
                    "Daily rate limit reached. Waiting..."
                );
                tokio::time::sleep(wait_until - now).await;
            }
        }

        // 5. Record timestamp
        let now = Instant::now();
        self.last_request = Some(now);
        self.hourly_window.push_back(now);
        self.daily_window.push_back(now);

        // 6. Record wait duration metric
        #[cfg(feature = "otel")]
        crate::metrics::record_rate_limit_wait("syoboi", wait_start);
    }

    /// Removes expired entries from sliding windows.
    fn cleanup_windows(&mut self, now: Instant) {
        let hour_ago = now.checked_sub(Duration::from_secs(3600));
        let day_ago = now.checked_sub(Duration::from_secs(86400));

        if let Some(hour_ago) = hour_ago {
            while self.hourly_window.front().is_some_and(|&t| t < hour_ago) {
                self.hourly_window.pop_front();
            }
        }

        if let Some(day_ago) = day_ago {
            while self.daily_window.front().is_some_and(|&t| t < day_ago) {
                self.daily_window.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_min_interval() {
        // Arrange
        let mut limiter = SyoboiRateLimiter::new(Duration::from_millis(50), 500, 10_000);

        // Act
        let start = Instant::now();
        limiter.wait().await;
        limiter.wait().await;
        let elapsed = start.elapsed();

        // Assert: second request should wait at least 50ms
        assert!(elapsed >= Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_rate_limiter_first_request_no_wait() {
        // Arrange
        let mut limiter = SyoboiRateLimiter::new(Duration::from_secs(1), 500, 10_000);

        // Act
        let start = Instant::now();
        limiter.wait().await;
        let elapsed = start.elapsed();

        // Assert: first request should pass immediately
        assert!(elapsed < Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_rate_limiter_records_timestamps() {
        // Arrange
        let mut limiter = SyoboiRateLimiter::new(Duration::from_millis(0), 500, 10_000);

        // Act
        limiter.wait().await;
        limiter.wait().await;
        limiter.wait().await;

        // Assert
        assert_eq!(limiter.hourly_window.len(), 3);
        assert_eq!(limiter.daily_window.len(), 3);
    }

    #[test]
    fn test_cleanup_windows_purges_expired() {
        // Arrange
        let mut limiter = SyoboiRateLimiter::new(Duration::from_secs(0), 500, 10_000);
        let now = Instant::now();
        // Insert timestamps from 2 hours ago (expired for hourly, not for daily)
        let two_hours_ago = now.checked_sub(Duration::from_secs(7200)).unwrap();
        limiter.hourly_window.push_back(two_hours_ago);
        limiter.daily_window.push_back(two_hours_ago);
        // Insert a recent timestamp
        limiter.hourly_window.push_back(now);
        limiter.daily_window.push_back(now);

        // Act
        limiter.cleanup_windows(now);

        // Assert: expired entry removed from hourly, but kept in daily
        assert_eq!(limiter.hourly_window.len(), 1);
        assert_eq!(limiter.daily_window.len(), 2);
    }

    #[tokio::test]
    async fn test_hourly_limit_fills_window() {
        // Arrange: tiny hourly limit
        let mut limiter = SyoboiRateLimiter::new(Duration::from_millis(0), 3, 10_000);

        // Act: fill to the limit
        limiter.wait().await;
        limiter.wait().await;
        limiter.wait().await;

        // Assert
        assert_eq!(limiter.hourly_window.len(), 3);
    }

    #[tokio::test]
    async fn test_daily_limit_fills_window() {
        // Arrange: tiny daily limit
        let mut limiter = SyoboiRateLimiter::new(Duration::from_millis(0), 10_000, 3);

        // Act: fill to the daily limit
        limiter.wait().await;
        limiter.wait().await;
        limiter.wait().await;

        // Assert
        assert_eq!(limiter.daily_window.len(), 3);
    }

    #[test]
    fn test_cleanup_windows_purges_daily_expired() {
        // Arrange
        let mut limiter = SyoboiRateLimiter::new(Duration::from_secs(0), 500, 10_000);
        let now = Instant::now();
        // Insert a timestamp from 2 days ago (expired for both hourly and daily)
        let two_days_ago = now.checked_sub(Duration::from_secs(172_800)).unwrap();
        limiter.hourly_window.push_back(two_days_ago);
        limiter.daily_window.push_back(two_days_ago);
        // Insert a recent timestamp
        limiter.hourly_window.push_back(now);
        limiter.daily_window.push_back(now);

        // Act
        limiter.cleanup_windows(now);

        // Assert: expired entry removed from both hourly and daily
        assert_eq!(limiter.hourly_window.len(), 1);
        assert_eq!(limiter.daily_window.len(), 1);
    }

    #[test]
    fn test_cleanup_windows_empty() {
        // Arrange: empty windows
        let mut limiter = SyoboiRateLimiter::new(Duration::from_secs(0), 500, 10_000);
        let now = Instant::now();

        // Act
        limiter.cleanup_windows(now);

        // Assert: no panic, windows still empty
        assert!(limiter.hourly_window.is_empty());
        assert!(limiter.daily_window.is_empty());
    }

    #[test]
    fn test_cleanup_windows_all_recent() {
        // Arrange: all timestamps are recent (nothing to purge)
        let mut limiter = SyoboiRateLimiter::new(Duration::from_secs(0), 500, 10_000);
        let now = Instant::now();
        limiter.hourly_window.push_back(now);
        limiter.hourly_window.push_back(now);
        limiter.daily_window.push_back(now);
        limiter.daily_window.push_back(now);

        // Act
        limiter.cleanup_windows(now);

        // Assert: nothing purged
        assert_eq!(limiter.hourly_window.len(), 2);
        assert_eq!(limiter.daily_window.len(), 2);
    }

    #[test]
    fn test_new_creates_empty_state() {
        // Arrange & Act
        let limiter = SyoboiRateLimiter::new(Duration::from_secs(1), 500, 10_000);

        // Assert
        assert!(limiter.last_request.is_none());
        assert!(limiter.hourly_window.is_empty());
        assert!(limiter.daily_window.is_empty());
        assert_eq!(limiter.min_interval, Duration::from_secs(1));
        assert_eq!(limiter.hourly_limit, 500);
        assert_eq!(limiter.daily_limit, 10_000);
    }

    #[tokio::test]
    async fn test_wait_updates_last_request() {
        // Arrange
        let mut limiter = SyoboiRateLimiter::new(Duration::from_millis(0), 500, 10_000);
        assert!(limiter.last_request.is_none());

        // Act
        limiter.wait().await;

        // Assert
        assert!(limiter.last_request.is_some());
    }

    #[test]
    fn test_cleanup_windows_checked_sub_none() {
        // Arrange: Instant close to epoch — checked_sub may return None
        // on some platforms, but we test the branch where it returns Some.
        let mut limiter = SyoboiRateLimiter::new(Duration::from_secs(0), 500, 10_000);
        let now = Instant::now();
        limiter.hourly_window.push_back(now);
        limiter.daily_window.push_back(now);

        // Act — should not panic even with very recent `now`
        limiter.cleanup_windows(now);

        // Assert
        assert_eq!(limiter.hourly_window.len(), 1);
        assert_eq!(limiter.daily_window.len(), 1);
    }

    #[tokio::test]
    async fn test_multiple_waits_accumulate_timestamps() {
        // Arrange
        let mut limiter = SyoboiRateLimiter::new(Duration::from_millis(0), 500, 10_000);

        // Act
        for _ in 0..5 {
            limiter.wait().await;
        }

        // Assert
        assert_eq!(limiter.hourly_window.len(), 5);
        assert_eq!(limiter.daily_window.len(), 5);
        assert!(limiter.last_request.is_some());
    }

    #[test]
    fn test_cleanup_multiple_expired_entries() {
        // Arrange: multiple expired entries in hourly window
        let mut limiter = SyoboiRateLimiter::new(Duration::from_secs(0), 500, 10_000);
        let now = Instant::now();
        let two_hours_ago = now.checked_sub(Duration::from_secs(7200)).unwrap();

        // Push 3 expired and 2 recent
        for _ in 0..3 {
            limiter.hourly_window.push_back(two_hours_ago);
        }
        for _ in 0..2 {
            limiter.hourly_window.push_back(now);
        }

        // Act
        limiter.cleanup_windows(now);

        // Assert — only 2 recent entries remain
        assert_eq!(limiter.hourly_window.len(), 2);
    }
}
