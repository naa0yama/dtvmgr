//! Syoboi Calendar API utility functions.

use std::collections::HashSet;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use regex::Regex;
use tracing::instrument;

use super::api::LocalSyoboiApi;
use super::params::{ProgLookupParams, TimeRange};
use super::types::SyoboiProgram;

/// Maximum number of programs returned per `ProgLookup` request.
const PROG_LOOKUP_LIMIT: usize = 5_000;

/// Regex for parsing `SubTitles` text.
#[allow(clippy::expect_used)]
static SUB_TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\*(\d+)\*(.+)").expect("failed to compile subtitle regex"));

/// Extracts episode number and subtitle pairs from raw `SubTitles` text.
///
/// # Input format
///
/// ```text
/// *01*Episode Title One
/// *02*Episode Title Two
/// ```
#[must_use]
pub fn parse_sub_titles(raw: &str) -> Vec<(u32, String)> {
    raw.lines()
        .filter_map(|line| {
            let caps = SUB_TITLE_RE.captures(line.trim())?;
            let count_str = caps.get(1)?.as_str();
            let count: u32 = match count_str.parse() {
                Ok(n) => n,
                Err(_) => return None,
            };
            let subtitle = caps.get(2)?.as_str().to_owned();
            Some((count, subtitle))
        })
        .collect()
}

/// Fetches all programs in the given time range, automatically paginating
/// when the API returns the maximum of 5,000 items per request.
///
/// Finds the maximum `StTime` in each page as a cursor to continue fetching.
/// Deduplicates results by `PID` to handle boundary overlaps.
///
/// # Errors
///
/// Returns an error if `params.range` is `None`, any underlying API request
/// fails, or timestamp conversion fails.
#[instrument(skip_all)]
pub async fn lookup_all_programs(
    api: &(impl LocalSyoboiApi + Sync),
    params: &ProgLookupParams,
) -> Result<Vec<SyoboiProgram>> {
    let original_range = params
        .range
        .as_ref()
        .context("ProgLookupParams.range is required for pagination")?;

    let end = original_range.end;
    let mut current_start = original_range.start;
    let mut all_programs: Vec<SyoboiProgram> = Vec::new();
    let mut seen_pids: HashSet<u32> = HashSet::new();
    let mut page: u32 = 0;

    loop {
        page = page.checked_add(1).context("page counter overflow")?;

        let page_range = TimeRange::new(current_start, end);
        let page_params = ProgLookupParams {
            range: Some(page_range.clone()),
            ..params.clone()
        };

        tracing::debug!(
            page = page,
            range = %page_range.to_syoboi_format(),
            ch_ids = ?page_params.ch_ids,
            tids = ?page_params.tids,
            "ProgLookup request"
        );

        let programs = api.lookup_programs(&page_params).await.with_context(|| {
            format!(
                "ProgLookup failed on page {page} (range: {})",
                page_range.to_syoboi_format()
            )
        })?;

        let fetched_count = programs.len();

        tracing::info!(
            page = page,
            fetched = fetched_count,
            range = %page_range.to_syoboi_format(),
            "ProgLookup page completed"
        );

        let dedup_before = all_programs.len();

        // Find the maximum st_time in this page before consuming the vec.
        // API does not guarantee st_time ordering, so we scan all items.
        let max_st_time = if fetched_count >= PROG_LOOKUP_LIMIT {
            programs
                .iter()
                .map(|p| p.st_time.as_str())
                .max()
                .map(String::from)
        } else {
            None
        };

        // Deduplicate and collect
        for prog in programs {
            if seen_pids.insert(prog.pid) {
                all_programs.push(prog);
            }
        }

        let new_count = all_programs.len().saturating_sub(dedup_before);
        let skipped = fetched_count.saturating_sub(new_count);
        if skipped > 0 {
            tracing::debug!(page = page, skipped = skipped, "duplicates removed");
        }

        // All data fetched if fewer than the limit
        if fetched_count < PROG_LOOKUP_LIMIT {
            break;
        }

        // Use the max st_time from this page as cursor for the next page
        let max_st_time = max_st_time.context("unexpected empty program list after limit check")?;

        let raw_result: std::result::Result<NaiveDateTime, _> =
            NaiveDateTime::parse_from_str(&max_st_time, "%Y-%m-%d %H:%M:%S");
        let next_start =
            raw_result.with_context(|| format!("invalid StTime for cursor: {max_st_time}"))?;

        // Guard against infinite loop: if cursor doesn't advance, stop
        if next_start <= current_start {
            tracing::warn!(
                cursor = %max_st_time,
                previous_start = %current_start,
                "cursor did not advance, stopping pagination"
            );
            break;
        }

        tracing::debug!(
            page = page,
            previous_start = %current_start,
            next_start = %next_start,
            "cursor advancing"
        );
        current_start = next_start;
    }

    tracing::info!(
        total = all_programs.len(),
        pages = page,
        "ProgLookup pagination completed"
    );

    Ok(all_programs)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]

    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU32, Ordering};

    use anyhow::Result;
    use chrono::NaiveDate;

    use super::*;
    use crate::syoboi::api::LocalSyoboiApi;
    use crate::syoboi::types::{SyoboiChannel, SyoboiChannelGroup, SyoboiTitle};

    /// Mock API that returns pre-configured batches in order.
    struct MockSyoboiApi {
        batches: Vec<Vec<SyoboiProgram>>,
        call_count: AtomicU32,
    }

    impl MockSyoboiApi {
        fn new(batches: Vec<Vec<SyoboiProgram>>) -> Self {
            Self {
                batches,
                call_count: AtomicU32::new(0),
            }
        }
    }

    impl LocalSyoboiApi for MockSyoboiApi {
        async fn lookup_titles(&self, _tids: &[u32]) -> Result<Vec<SyoboiTitle>> {
            Ok(vec![])
        }

        async fn lookup_programs(&self, _params: &ProgLookupParams) -> Result<Vec<SyoboiProgram>> {
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
            #[allow(clippy::as_conversions)]
            let idx = idx as usize;
            if idx < self.batches.len() {
                Ok(self.batches[idx].clone())
            } else {
                Ok(vec![])
            }
        }

        async fn lookup_channels(&self, _ch_ids: Option<&[u32]>) -> Result<Vec<SyoboiChannel>> {
            Ok(vec![])
        }

        async fn lookup_channel_groups(
            &self,
            _ch_gids: Option<&[u32]>,
        ) -> Result<Vec<SyoboiChannelGroup>> {
            Ok(vec![])
        }
    }

    /// Helper to create a minimal `SyoboiProgram`.
    fn make_program(pid: u32, st_time: &str) -> SyoboiProgram {
        SyoboiProgram {
            pid,
            tid: 1,
            st_time: String::from(st_time),
            st_offset: None,
            ed_time: String::from("2024-01-01 00:30:00"),
            count: None,
            sub_title: None,
            prog_comment: None,
            flag: None,
            deleted: None,
            warn: None,
            ch_id: 1,
            revision: None,
            last_update: None,
            st_sub_title: None,
        }
    }

    fn make_range(start: (i32, u32, u32), end: (i32, u32, u32)) -> TimeRange {
        TimeRange::new(
            NaiveDate::from_ymd_opt(start.0, start.1, start.2)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            NaiveDate::from_ymd_opt(end.0, end.1, end.2)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        )
    }

    #[test]
    fn test_parse_sub_titles() {
        // Arrange
        let raw = "*01*オペレーション〈梟(ストリクス)〉\n*02*妻役を確保せよ\n*03*受験対策をせよ";

        // Act
        let result = parse_sub_titles(raw);

        // Assert
        assert_eq!(result.len(), 3);
        assert_eq!(
            result[0],
            (1, String::from("オペレーション〈梟(ストリクス)〉"))
        );
        assert_eq!(result[1], (2, String::from("妻役を確保せよ")));
        assert_eq!(result[2], (3, String::from("受験対策をせよ")));
    }

    #[test]
    fn test_parse_sub_titles_empty() {
        // Arrange & Act
        let result = parse_sub_titles("");

        // Assert
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_lookup_all_programs_single_page() {
        // Arrange
        let programs = vec![
            make_program(1, "2024-01-01 00:00:00"),
            make_program(2, "2024-01-01 01:00:00"),
            make_program(3, "2024-01-01 02:00:00"),
        ];
        let mock = MockSyoboiApi::new(vec![programs]);
        let params = ProgLookupParams {
            range: Some(make_range((2024, 1, 1), (2024, 2, 1))),
            ..ProgLookupParams::default()
        };

        // Act
        let result = lookup_all_programs(&mock, &params).await.unwrap();

        // Assert
        assert_eq!(result.len(), 3);
        assert_eq!(mock.call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_lookup_all_programs_two_pages() {
        // Arrange: first batch = 5000 items, second batch < 5000
        // Max st_time is in the middle, not the last item (order not guaranteed)
        let mut batch1: Vec<SyoboiProgram> = (1..=5000)
            .map(|i| make_program(i, "2024-01-15 12:00:00"))
            .collect();
        batch1[2500].st_time = String::from("2024-01-20 00:00:00");

        let batch2 = vec![
            make_program(5001, "2024-01-20 01:00:00"),
            make_program(5002, "2024-01-20 02:00:00"),
        ];
        let mock = MockSyoboiApi::new(vec![batch1, batch2]);
        let params = ProgLookupParams {
            range: Some(make_range((2024, 1, 1), (2024, 2, 1))),
            ..ProgLookupParams::default()
        };

        // Act
        let result = lookup_all_programs(&mock, &params).await.unwrap();

        // Assert
        assert_eq!(result.len(), 5002);
        assert_eq!(mock.call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_lookup_all_programs_deduplication() {
        // Arrange: second batch contains boundary duplicates
        // Max st_time is at an arbitrary position (order not guaranteed)
        let mut batch1: Vec<SyoboiProgram> = (1..=5000)
            .map(|i| make_program(i, "2024-01-15 12:00:00"))
            .collect();
        batch1[100].st_time = String::from("2024-01-20 00:00:00");

        let batch2 = vec![
            make_program(4999, "2024-01-20 00:00:00"), // duplicate
            make_program(5000, "2024-01-20 00:00:00"), // duplicate
            make_program(5001, "2024-01-30 00:00:00"), // new
        ];
        let mock = MockSyoboiApi::new(vec![batch1, batch2]);
        let params = ProgLookupParams {
            range: Some(make_range((2024, 1, 1), (2024, 2, 1))),
            ..ProgLookupParams::default()
        };

        // Act
        let result = lookup_all_programs(&mock, &params).await.unwrap();

        // Assert: 5000 from batch1 + 1 new from batch2 = 5001
        assert_eq!(result.len(), 5001);
        let pids: HashSet<u32> = result.iter().map(|p| p.pid).collect();
        assert!(pids.contains(&5001));
    }

    #[tokio::test]
    async fn test_lookup_all_programs_requires_range() {
        // Arrange
        let mock = MockSyoboiApi::new(vec![]);
        let params = ProgLookupParams::default(); // range is None

        // Act
        let result = lookup_all_programs(&mock, &params).await;

        // Assert
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("range is required")
        );
    }

    #[tokio::test]
    async fn test_lookup_all_programs_stops_when_cursor_stalls() {
        // Arrange: all 5000 items share the same st_time as the range start
        let batch: Vec<SyoboiProgram> = (1..=5000)
            .map(|i| make_program(i, "2024-01-15 00:00:00"))
            .collect();
        let mock = MockSyoboiApi::new(vec![batch]);
        let params = ProgLookupParams {
            range: Some(make_range((2024, 1, 15), (2024, 2, 1))),
            ..ProgLookupParams::default()
        };

        // Act
        let result = lookup_all_programs(&mock, &params).await.unwrap();

        // Assert: returns what we got without infinite loop
        assert_eq!(result.len(), 5000);
        assert_eq!(mock.call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_lookup_all_programs_empty_response() {
        // Arrange
        let mock = MockSyoboiApi::new(vec![vec![]]);
        let params = ProgLookupParams {
            range: Some(make_range((2024, 1, 1), (2024, 2, 1))),
            ..ProgLookupParams::default()
        };

        // Act
        let result = lookup_all_programs(&mock, &params).await.unwrap();

        // Assert
        assert!(result.is_empty());
    }
}
