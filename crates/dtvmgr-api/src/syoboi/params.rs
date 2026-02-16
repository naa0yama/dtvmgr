//! Syoboi Calendar API request parameter types.

use anyhow::{Context, Result, bail};
use chrono::{Duration, Local, NaiveDateTime};

/// `Range` parameter for `ProgLookup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeRange {
    /// Start datetime.
    pub start: NaiveDateTime,
    /// End datetime.
    pub end: NaiveDateTime,
}

impl TimeRange {
    /// Creates a new `TimeRange`.
    #[must_use]
    pub const fn new(start: NaiveDateTime, end: NaiveDateTime) -> Self {
        Self { start, end }
    }

    /// Formats as Syoboi `Range` string.
    ///
    /// Example: `"20240101_000000-20240201_000000"`
    #[must_use]
    pub fn to_syoboi_format(&self) -> String {
        format!(
            "{}-{}",
            self.start.format("%Y%m%d_%H%M%S"),
            self.end.format("%Y%m%d_%H%M%S"),
        )
    }
}

/// Request parameters for `ProgLookup`.
#[derive(Debug, Clone)]
pub struct ProgLookupParams {
    /// Title ID filter (`None` = all titles).
    pub tids: Option<Vec<u32>>,
    /// Channel ID filter (`None` = all channels).
    pub ch_ids: Option<Vec<u32>>,
    /// Time range (`Range` parameter).
    pub range: Option<TimeRange>,
    /// Start time filter (`StTime` parameter).
    pub st_time: Option<String>,
    /// Last update filter.
    pub last_update: Option<String>,
    /// Join `SubTitles` table (default: `true`).
    pub join_sub_titles: bool,
    /// Restrict output fields.
    pub fields: Option<Vec<String>>,
}

impl Default for ProgLookupParams {
    fn default() -> Self {
        Self {
            tids: None,
            ch_ids: None,
            range: None,
            st_time: None,
            last_update: None,
            join_sub_titles: true,
            fields: None,
        }
    }
}

/// Tries full datetime formats, returns `None` if both fail.
fn try_full_datetime(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .ok()
}

/// Converts a datetime string for `--time-since` (date-only defaults to `00:00:00`).
///
/// Accepts: `%Y-%m-%dT%H:%M:%S`, `%Y-%m-%d %H:%M:%S`, `%Y-%m-%d`.
///
/// # Errors
///
/// Returns an error if the string does not match any known format.
pub fn to_naive_datetime_since(s: &str) -> Result<NaiveDateTime> {
    if let Some(dt) = try_full_datetime(s) {
        return Ok(dt);
    }
    NaiveDateTime::parse_from_str(&format!("{s}T00:00:00"), "%Y-%m-%dT%H:%M:%S")
        .with_context(|| format!("invalid datetime format: {s}"))
}

/// Converts a datetime string for `--time-until` (date-only defaults to `23:59:59`).
///
/// Accepts: `%Y-%m-%dT%H:%M:%S`, `%Y-%m-%d %H:%M:%S`, `%Y-%m-%d`.
///
/// # Errors
///
/// Returns an error if the string does not match any known format.
pub fn to_naive_datetime_until(s: &str) -> Result<NaiveDateTime> {
    if let Some(dt) = try_full_datetime(s) {
        return Ok(dt);
    }
    NaiveDateTime::parse_from_str(&format!("{s}T23:59:59"), "%Y-%m-%dT%H:%M:%S")
        .with_context(|| format!("invalid datetime format: {s}"))
}

/// Resolves time range from optional since/until strings using local timezone.
///
/// When both are `None`, defaults to `[now - 1 day, now + 1 day]`.
///
/// # Errors
///
/// Returns an error if only one of since/until is specified,
/// or if datetime parsing fails.
pub fn resolve_time_range(time_since: Option<&str>, time_until: Option<&str>) -> Result<TimeRange> {
    match (time_since, time_until) {
        (None, None) => {
            let now = Local::now().naive_local();
            let start = now
                .checked_sub_signed(Duration::days(1))
                .context("failed to compute start time")?;
            let end = now
                .checked_add_signed(Duration::days(1))
                .context("failed to compute end time")?;
            Ok(TimeRange::new(start, end))
        }
        (Some(since), Some(until)) => {
            let start = to_naive_datetime_since(since)?;
            let end = to_naive_datetime_until(until)?;
            Ok(TimeRange::new(start, end))
        }
        _ => {
            bail!("both --time-since and --time-until must be specified together");
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use chrono::NaiveDate;

    use super::*;

    #[test]
    fn test_time_range_format() {
        // Arrange
        let start = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 2, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();

        // Act
        let range = TimeRange::new(start, end);

        // Assert
        assert_eq!(range.to_syoboi_format(), "20240101_000000-20240201_000000");
    }

    #[test]
    fn test_prog_lookup_params_default() {
        // Arrange & Act
        let params = ProgLookupParams::default();

        // Assert
        assert!(params.tids.is_none());
        assert!(params.ch_ids.is_none());
        assert!(params.range.is_none());
        assert!(params.join_sub_titles);
        assert!(params.fields.is_none());
    }

    #[test]
    fn test_to_naive_datetime_since_iso_format() {
        // Arrange & Act
        let dt = to_naive_datetime_since("2024-01-15T09:30:00").unwrap();

        // Assert
        assert_eq!(dt.to_string(), "2024-01-15 09:30:00");
    }

    #[test]
    fn test_to_naive_datetime_until_iso_format() {
        // Arrange & Act
        let dt = to_naive_datetime_until("2024-01-15T09:30:00").unwrap();

        // Assert
        assert_eq!(dt.to_string(), "2024-01-15 09:30:00");
    }

    #[test]
    fn test_to_naive_datetime_since_space_format() {
        // Arrange & Act
        let dt = to_naive_datetime_since("2024-01-15 09:30:00").unwrap();

        // Assert
        assert_eq!(dt.to_string(), "2024-01-15 09:30:00");
    }

    #[test]
    fn test_to_naive_datetime_until_space_format() {
        // Arrange & Act
        let dt = to_naive_datetime_until("2024-01-15 09:30:00").unwrap();

        // Assert
        assert_eq!(dt.to_string(), "2024-01-15 09:30:00");
    }

    #[test]
    fn test_to_naive_datetime_since_date_only() {
        // Arrange & Act
        let dt = to_naive_datetime_since("2024-01-15").unwrap();

        // Assert
        assert_eq!(dt.to_string(), "2024-01-15 00:00:00");
    }

    #[test]
    fn test_to_naive_datetime_until_date_only() {
        // Arrange & Act
        let dt = to_naive_datetime_until("2024-01-15").unwrap();

        // Assert
        assert_eq!(dt.to_string(), "2024-01-15 23:59:59");
    }

    #[test]
    fn test_to_naive_datetime_since_invalid() {
        // Arrange & Act
        let result = to_naive_datetime_since("not-a-date");

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn test_to_naive_datetime_until_invalid() {
        // Arrange & Act
        let result = to_naive_datetime_until("not-a-date");

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_time_range_both_none() {
        // Arrange & Act
        let range = resolve_time_range(None, None).unwrap();

        // Assert: range should span roughly 2 days
        let diff = range.end - range.start;
        assert_eq!(diff.num_days(), 2);
    }

    #[test]
    fn test_resolve_time_range_both_some() {
        // Arrange & Act
        let range = resolve_time_range(Some("2024-01-01"), Some("2024-01-31")).unwrap();

        // Assert
        assert_eq!(range.start.to_string(), "2024-01-01 00:00:00");
        assert_eq!(range.end.to_string(), "2024-01-31 23:59:59");
    }

    #[test]
    fn test_resolve_time_range_only_since() {
        // Arrange & Act
        let result = resolve_time_range(Some("2024-01-01"), None);

        // Assert
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("both --time-since and --time-until must be specified together")
        );
    }

    #[test]
    fn test_resolve_time_range_only_until() {
        // Arrange & Act
        let result = resolve_time_range(None, Some("2024-01-31"));

        // Assert
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("both --time-since and --time-until must be specified together")
        );
    }
}
