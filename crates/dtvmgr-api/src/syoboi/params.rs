//! Syoboi Calendar API request parameter types.

use chrono::NaiveDateTime;

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
}
