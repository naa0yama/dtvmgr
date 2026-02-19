//! Title cache CRUD operations.

use anyhow::{Context, Result};
use rusqlite::Connection;

/// A cached title with optional TMDB mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedTitle {
    /// Syoboi title ID.
    pub tid: u32,
    /// Mapped TMDB series ID (cache, nullable).
    pub tmdb_series_id: Option<u64>,
    /// Mapped TMDB season number (cache, nullable).
    pub tmdb_season_number: Option<u32>,
    /// Title name.
    pub title: String,
    /// Short title (nullable).
    pub short_title: Option<String>,
    /// Title reading in hiragana (nullable).
    pub title_yomi: Option<String>,
    /// English title (nullable).
    pub title_en: Option<String>,
    /// Category ID (nullable).
    pub cat: Option<u32>,
    /// Title flag (nullable).
    pub title_flag: Option<u32>,
    /// First broadcast year (nullable).
    pub first_year: Option<u32>,
    /// First broadcast month (nullable).
    pub first_month: Option<u32>,
    /// Keywords parsed from comma-separated DB value.
    pub keywords: Vec<String>,
    /// Raw subtitle text (nullable).
    pub sub_titles: Option<String>,
    /// Last update timestamp.
    pub last_update: String,
    /// TMDB original name from search result (nullable).
    pub tmdb_original_name: Option<String>,
    /// TMDB localized name from search result (nullable).
    pub tmdb_name: Option<String>,
    /// TMDB alternative titles as JSON array (nullable).
    pub tmdb_alt_titles: Option<String>,
    /// UTC timestamp of last TMDB lookup attempt (nullable).
    pub tmdb_last_updated: Option<String>,
}

/// Parses a comma-separated keyword string into a Vec, filtering empty entries.
#[must_use]
pub fn parse_keywords(raw: Option<String>) -> Vec<String> {
    raw.map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(String::from)
            .collect()
    })
    .unwrap_or_default()
}

/// Serializes keywords into comma-separated string for DB storage.
/// Returns None for empty Vec (stored as NULL).
fn serialize_keywords(keywords: &[String]) -> Option<String> {
    if keywords.is_empty() {
        None
    } else {
        Some(keywords.join(","))
    }
}

/// Prefixes indicating non-searchable keywords.
const KEYWORD_EXCLUDE_PREFIXES: &[&str] = &["wikipedia:", "wikiedia:", "legwork:"];

/// Filters keywords for TMDB search, removing:
/// - Keywords with exclusion prefixes (wikipedia:, legwork:, etc.)
/// - Empty / whitespace-only entries
/// - Keywords exactly matching title or `short_title`
#[must_use]
pub fn filter_keywords(keywords: &[String], title: &str, short_title: Option<&str>) -> Vec<String> {
    keywords
        .iter()
        .filter(|kw| {
            let trimmed = kw.trim();
            if trimmed.is_empty() {
                return false;
            }
            let lower = trimmed.to_lowercase();
            for prefix in KEYWORD_EXCLUDE_PREFIXES {
                if lower.starts_with(prefix) {
                    return false;
                }
            }
            if trimmed == title {
                return false;
            }
            if let Some(st) = short_title
                && trimmed == st
            {
                return false;
            }
            true
        })
        .map(|kw| kw.trim().to_owned())
        .collect()
}

/// Upserts titles into the cache. Returns the number of rows changed.
///
/// Uses `INSERT ... ON CONFLICT(tid) DO UPDATE SET` to update existing rows.
/// TMDB mapping columns (`tmdb_series_id`, `tmdb_season_number`) are preserved
/// on conflict to avoid overwriting manual mappings.
/// Only updates when `last_update` has changed.
///
/// # Errors
///
/// Returns an error if the database operation fails.
#[allow(clippy::module_name_repetitions)]
pub fn upsert_titles(conn: &Connection, titles: &[CachedTitle]) -> Result<usize> {
    let tx = conn
        .unchecked_transaction()
        .context("failed to begin transaction")?;

    let mut stmt = tx
        .prepare(
            "INSERT INTO titles (
                tid, tmdb_series_id, tmdb_season_number,
                title, short_title, title_yomi, title_en,
                cat, title_flag, first_year, first_month,
                keywords, sub_titles, last_update,
                tmdb_original_name, tmdb_name, tmdb_alt_titles,
                tmdb_last_updated
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            ON CONFLICT(tid) DO UPDATE SET
                title = excluded.title,
                short_title = excluded.short_title,
                title_yomi = excluded.title_yomi,
                title_en = excluded.title_en,
                cat = excluded.cat,
                title_flag = excluded.title_flag,
                first_year = excluded.first_year,
                first_month = excluded.first_month,
                keywords = excluded.keywords,
                sub_titles = excluded.sub_titles,
                last_update = excluded.last_update
            WHERE titles.last_update != excluded.last_update",
        )
        .context("failed to prepare titles upsert")?;

    let mut changed: usize = 0;
    for t in titles {
        let rows = stmt
            .execute(rusqlite::params![
                t.tid,
                t.tmdb_series_id,
                t.tmdb_season_number,
                t.title,
                t.short_title,
                t.title_yomi,
                t.title_en,
                t.cat,
                t.title_flag,
                t.first_year,
                t.first_month,
                serialize_keywords(&t.keywords),
                t.sub_titles,
                t.last_update,
                t.tmdb_original_name,
                t.tmdb_name,
                t.tmdb_alt_titles,
                t.tmdb_last_updated,
            ])
            .with_context(|| format!("failed to upsert title {}", t.tid))?;
        changed = changed.saturating_add(rows);
    }

    drop(stmt);
    tx.commit().context("failed to commit titles upsert")?;
    Ok(changed)
}

/// Loads all titles from the cache.
///
/// # Errors
///
/// Returns an error if the database query fails.
#[allow(clippy::module_name_repetitions)]
pub fn load_titles(conn: &Connection) -> Result<Vec<CachedTitle>> {
    let mut stmt = conn
        .prepare(
            "SELECT tid, tmdb_series_id, tmdb_season_number,
                    title, short_title, title_yomi, title_en,
                    cat, title_flag, first_year, first_month,
                    keywords, sub_titles, last_update,
                    tmdb_original_name, tmdb_name, tmdb_alt_titles,
                    tmdb_last_updated
             FROM titles
             ORDER BY tid",
        )
        .context("failed to prepare titles query")?;

    let rows = stmt
        .query_map([], |row| {
            Ok(CachedTitle {
                tid: row.get(0)?,
                tmdb_series_id: row.get(1)?,
                tmdb_season_number: row.get(2)?,
                title: row.get(3)?,
                short_title: row.get(4)?,
                title_yomi: row.get(5)?,
                title_en: row.get(6)?,
                cat: row.get(7)?,
                title_flag: row.get(8)?,
                first_year: row.get(9)?,
                first_month: row.get(10)?,
                keywords: parse_keywords(row.get(11)?),
                sub_titles: row.get(12)?,
                last_update: row.get(13)?,
                tmdb_original_name: row.get(14)?,
                tmdb_name: row.get(15)?,
                tmdb_alt_titles: row.get(16)?,
                tmdb_last_updated: row.get(17)?,
            })
        })
        .context("failed to query titles")?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read titles rows")
}

/// Loads titles by TID filter.
///
/// # Errors
///
/// Returns an error if the database query fails.
#[allow(clippy::module_name_repetitions)]
pub fn load_titles_by_tids(conn: &Connection, tids: &[u32]) -> Result<Vec<CachedTitle>> {
    if tids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = tids.iter().map(|_| String::from("?")).collect();
    let sql = format!(
        "SELECT tid, tmdb_series_id, tmdb_season_number,
                title, short_title, title_yomi, title_en,
                cat, title_flag, first_year, first_month,
                keywords, sub_titles, last_update,
                tmdb_original_name, tmdb_name, tmdb_alt_titles,
                tmdb_last_updated
         FROM titles
         WHERE tid IN ({})
         ORDER BY tid",
        placeholders.join(", ")
    );

    let mut stmt = conn
        .prepare(&sql)
        .context("failed to prepare titles query")?;

    let params: Vec<Box<dyn rusqlite::types::ToSql>> = tids
        .iter()
        .map(|tid| -> Box<dyn rusqlite::types::ToSql> { Box::new(*tid) })
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(AsRef::as_ref).collect();

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(CachedTitle {
                tid: row.get(0)?,
                tmdb_series_id: row.get(1)?,
                tmdb_season_number: row.get(2)?,
                title: row.get(3)?,
                short_title: row.get(4)?,
                title_yomi: row.get(5)?,
                title_en: row.get(6)?,
                cat: row.get(7)?,
                title_flag: row.get(8)?,
                first_year: row.get(9)?,
                first_month: row.get(10)?,
                keywords: parse_keywords(row.get(11)?),
                sub_titles: row.get(12)?,
                last_update: row.get(13)?,
                tmdb_original_name: row.get(14)?,
                tmdb_name: row.get(15)?,
                tmdb_alt_titles: row.get(16)?,
                tmdb_last_updated: row.get(17)?,
            })
        })
        .context("failed to query titles by tids")?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read titles rows")
}

/// Updates TMDB mapping for a title.
///
/// # Errors
///
/// Returns an error if the database operation fails.
pub fn update_tmdb_mapping(
    conn: &Connection,
    tid: u32,
    tmdb_series_id: Option<u64>,
    tmdb_season_number: Option<u32>,
) -> Result<()> {
    conn.execute(
        "UPDATE titles SET tmdb_series_id = ?1, tmdb_season_number = ?2 WHERE tid = ?3",
        rusqlite::params![tmdb_series_id, tmdb_season_number, tid],
    )
    .with_context(|| format!("failed to update TMDB mapping for title {tid}"))?;
    Ok(())
}

/// Updates TMDB search result fields for a title.
///
/// Sets `tmdb_series_id`, `tmdb_original_name`, `tmdb_name`,
/// `tmdb_alt_titles`, and `tmdb_last_updated` in a single UPDATE.
///
/// # Errors
///
/// Returns an error if the database operation fails.
pub fn update_tmdb_search_result(
    conn: &Connection,
    tid: u32,
    tmdb_series_id: u64,
    tmdb_original_name: &str,
    tmdb_name: &str,
    tmdb_alt_titles: &str,
    tmdb_last_updated: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE titles
         SET tmdb_series_id = ?1,
             tmdb_original_name = ?2,
             tmdb_name = ?3,
             tmdb_alt_titles = ?4,
             tmdb_last_updated = ?5
         WHERE tid = ?6",
        rusqlite::params![
            tmdb_series_id,
            tmdb_original_name,
            tmdb_name,
            tmdb_alt_titles,
            tmdb_last_updated,
            tid
        ],
    )
    .with_context(|| format!("failed to update TMDB search result for title {tid}"))?;
    Ok(())
}

/// Updates only the `tmdb_last_updated` timestamp for a title.
///
/// Used for Skipped/Error outcomes to record the lookup attempt time
/// without modifying other TMDB fields.
///
/// # Errors
///
/// Returns an error if the database operation fails.
pub fn update_tmdb_last_updated(conn: &Connection, tid: u32, timestamp: &str) -> Result<()> {
    conn.execute(
        "UPDATE titles SET tmdb_last_updated = ?1 WHERE tid = ?2",
        rusqlite::params![timestamp, tid],
    )
    .with_context(|| format!("failed to update tmdb_last_updated for title {tid}"))?;
    Ok(())
}

/// Deletes titles whose `cat` is not in the allowed set. Returns the number of rows deleted.
///
/// Titles with `cat IS NULL` are also deleted.
///
/// # Errors
///
/// Returns an error if the database operation fails.
pub fn delete_titles_by_cat_not_in(conn: &Connection, allowed_cats: &[u32]) -> Result<usize> {
    if allowed_cats.is_empty() {
        let deleted = conn
            .execute("DELETE FROM titles", [])
            .context("failed to delete all titles")?;
        return Ok(deleted);
    }

    let placeholders: Vec<String> = allowed_cats.iter().map(|_| String::from("?")).collect();
    let sql = format!(
        "DELETE FROM titles WHERE cat IS NULL OR cat NOT IN ({})",
        placeholders.join(", ")
    );

    let params: Vec<Box<dyn rusqlite::types::ToSql>> = allowed_cats
        .iter()
        .map(|c| -> Box<dyn rusqlite::types::ToSql> { Box::new(*c) })
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(AsRef::as_ref).collect();

    let deleted = conn
        .execute(&sql, param_refs.as_slice())
        .context("failed to delete titles by cat filter")?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use crate::connection::open_db;

    fn setup_db() -> (Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(Some(&dir.path().to_path_buf())).unwrap();
        (conn, dir)
    }

    fn make_title(tid: u32, title: &str, last_update: &str) -> CachedTitle {
        CachedTitle {
            tid,
            tmdb_series_id: None,
            tmdb_season_number: None,
            title: String::from(title),
            short_title: None,
            title_yomi: None,
            title_en: None,
            cat: None,
            title_flag: None,
            first_year: None,
            first_month: None,
            keywords: Vec::new(),
            sub_titles: None,
            last_update: String::from(last_update),
            tmdb_original_name: None,
            tmdb_name: None,
            tmdb_alt_titles: None,
            tmdb_last_updated: None,
        }
    }

    #[test]
    fn test_upsert_and_load_titles() {
        // Arrange
        let (conn, _dir) = setup_db();
        let titles = vec![
            make_title(100, "Test Title A", "2024-01-01 00:00:00"),
            make_title(200, "Test Title B", "2024-01-02 00:00:00"),
        ];

        // Act
        let changed = upsert_titles(&conn, &titles).unwrap();
        let loaded = load_titles(&conn).unwrap();

        // Assert
        assert_eq!(changed, 2);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].tid, 100);
        assert_eq!(loaded[0].title, "Test Title A");
        assert_eq!(loaded[1].tid, 200);
    }

    #[test]
    fn test_upsert_updates_on_last_update_change() {
        // Arrange
        let (conn, _dir) = setup_db();
        let titles = vec![make_title(100, "Original", "2024-01-01 00:00:00")];
        upsert_titles(&conn, &titles).unwrap();

        // Act: upsert with new last_update
        let updated = vec![make_title(100, "Updated", "2024-02-01 00:00:00")];
        let changed = upsert_titles(&conn, &updated).unwrap();
        let loaded = load_titles(&conn).unwrap();

        // Assert
        assert_eq!(changed, 1);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title, "Updated");
        assert_eq!(loaded[0].last_update, "2024-02-01 00:00:00");
    }

    #[test]
    fn test_upsert_skips_when_last_update_unchanged() {
        // Arrange
        let (conn, _dir) = setup_db();
        let titles = vec![make_title(100, "Original", "2024-01-01 00:00:00")];
        upsert_titles(&conn, &titles).unwrap();

        // Act: upsert with same last_update but different title
        let same_update = vec![make_title(100, "Should Not Update", "2024-01-01 00:00:00")];
        let changed = upsert_titles(&conn, &same_update).unwrap();
        let loaded = load_titles(&conn).unwrap();

        // Assert: title should remain "Original", 0 rows changed
        assert_eq!(changed, 0);
        assert_eq!(loaded[0].title, "Original");
    }

    #[test]
    fn test_upsert_preserves_tmdb_mapping() {
        // Arrange
        let (conn, _dir) = setup_db();
        let titles = vec![make_title(100, "Test", "2024-01-01 00:00:00")];
        upsert_titles(&conn, &titles).unwrap();

        // Set TMDB mapping
        update_tmdb_mapping(&conn, 100, Some(12345), Some(1)).unwrap();

        // Act: upsert with new last_update (TMDB fields should be preserved)
        let updated = vec![make_title(100, "Updated", "2024-02-01 00:00:00")];
        upsert_titles(&conn, &updated).unwrap();
        let loaded = load_titles(&conn).unwrap();

        // Assert: TMDB mapping preserved
        assert_eq!(loaded[0].title, "Updated");
        assert_eq!(loaded[0].tmdb_series_id, Some(12345));
        assert_eq!(loaded[0].tmdb_season_number, Some(1));
    }

    #[test]
    fn test_load_titles_by_tids() {
        // Arrange
        let (conn, _dir) = setup_db();
        let titles = vec![
            make_title(100, "Title A", "2024-01-01 00:00:00"),
            make_title(200, "Title B", "2024-01-02 00:00:00"),
            make_title(300, "Title C", "2024-01-03 00:00:00"),
        ];
        upsert_titles(&conn, &titles).unwrap();

        // Act
        let loaded = load_titles_by_tids(&conn, &[100, 300]).unwrap();

        // Assert
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].tid, 100);
        assert_eq!(loaded[1].tid, 300);
    }

    #[test]
    fn test_load_titles_by_tids_empty() {
        // Arrange
        let (conn, _dir) = setup_db();

        // Act
        let loaded = load_titles_by_tids(&conn, &[]).unwrap();

        // Assert
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_update_tmdb_mapping() {
        // Arrange
        let (conn, _dir) = setup_db();
        let titles = vec![make_title(100, "Test", "2024-01-01 00:00:00")];
        upsert_titles(&conn, &titles).unwrap();

        // Act
        update_tmdb_mapping(&conn, 100, Some(99999), Some(2)).unwrap();
        let loaded = load_titles(&conn).unwrap();

        // Assert
        assert_eq!(loaded[0].tmdb_series_id, Some(99999));
        assert_eq!(loaded[0].tmdb_season_number, Some(2));
    }

    #[test]
    fn test_update_tmdb_search_result() {
        // Arrange
        let (conn, _dir) = setup_db();
        let titles = vec![make_title(100, "ルパン三世", "2024-01-01 00:00:00")];
        upsert_titles(&conn, &titles).unwrap();

        let alt_titles_json = r#"[{"iso_3166_1":"JP","title":"Lupin III","type":"romaji"}]"#;

        // Act
        update_tmdb_search_result(
            &conn,
            100,
            31572,
            "ルパン三世",
            "ルパン三世",
            alt_titles_json,
            "2026-02-19T10:30:00Z",
        )
        .unwrap();
        let loaded = load_titles(&conn).unwrap();

        // Assert
        assert_eq!(loaded[0].tmdb_series_id, Some(31572));
        assert_eq!(loaded[0].tmdb_original_name.as_deref(), Some("ルパン三世"));
        assert_eq!(loaded[0].tmdb_name.as_deref(), Some("ルパン三世"));
        assert_eq!(loaded[0].tmdb_alt_titles.as_deref(), Some(alt_titles_json));
        assert_eq!(
            loaded[0].tmdb_last_updated.as_deref(),
            Some("2026-02-19T10:30:00Z")
        );
    }

    #[test]
    fn test_upsert_preserves_tmdb_search_result() {
        // Arrange
        let (conn, _dir) = setup_db();
        let titles = vec![make_title(100, "Test", "2024-01-01 00:00:00")];
        upsert_titles(&conn, &titles).unwrap();

        // Set TMDB search result
        update_tmdb_search_result(
            &conn,
            100,
            31572,
            "Original",
            "Name",
            "[]",
            "2026-02-19T10:30:00Z",
        )
        .unwrap();

        // Act: upsert with new last_update (TMDB fields should be preserved)
        let updated = vec![make_title(100, "Updated", "2024-02-01 00:00:00")];
        upsert_titles(&conn, &updated).unwrap();
        let loaded = load_titles(&conn).unwrap();

        // Assert: TMDB search result preserved
        assert_eq!(loaded[0].title, "Updated");
        assert_eq!(loaded[0].tmdb_series_id, Some(31572));
        assert_eq!(loaded[0].tmdb_original_name.as_deref(), Some("Original"));
        assert_eq!(loaded[0].tmdb_name.as_deref(), Some("Name"));
        assert_eq!(loaded[0].tmdb_alt_titles.as_deref(), Some("[]"));
    }

    fn make_title_with_cat(tid: u32, title: &str, cat: Option<u32>) -> CachedTitle {
        CachedTitle {
            cat,
            ..make_title(tid, title, "2024-01-01 00:00:00")
        }
    }

    #[test]
    fn test_delete_titles_by_cat_not_in() {
        // Arrange
        let (conn, _dir) = setup_db();
        let titles = vec![
            make_title_with_cat(1, "Anime", Some(1)),
            make_title_with_cat(2, "OVA", Some(7)),
            make_title_with_cat(3, "Radio", Some(2)),
            make_title_with_cat(4, "NoCat", None),
        ];
        upsert_titles(&conn, &titles).unwrap();

        // Act
        let deleted = delete_titles_by_cat_not_in(&conn, &[1, 7]).unwrap();
        let remaining = load_titles(&conn).unwrap();

        // Assert
        assert_eq!(deleted, 2);
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].tid, 1);
        assert_eq!(remaining[1].tid, 2);
    }

    #[test]
    fn test_delete_titles_by_cat_not_in_empty_allowed() {
        // Arrange
        let (conn, _dir) = setup_db();
        let titles = vec![
            make_title_with_cat(1, "Anime", Some(1)),
            make_title_with_cat(2, "OVA", Some(7)),
        ];
        upsert_titles(&conn, &titles).unwrap();

        // Act
        let deleted = delete_titles_by_cat_not_in(&conn, &[]).unwrap();
        let remaining = load_titles(&conn).unwrap();

        // Assert
        assert_eq!(deleted, 2);
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_update_tmdb_last_updated() {
        // Arrange
        let (conn, _dir) = setup_db();
        let titles = vec![make_title(100, "Test", "2024-01-01 00:00:00")];
        upsert_titles(&conn, &titles).unwrap();

        // Act
        update_tmdb_last_updated(&conn, 100, "2026-02-19T12:00:00Z").unwrap();
        let loaded = load_titles(&conn).unwrap();

        // Assert
        assert_eq!(
            loaded[0].tmdb_last_updated.as_deref(),
            Some("2026-02-19T12:00:00Z")
        );
        // Other TMDB fields remain None
        assert_eq!(loaded[0].tmdb_series_id, None);
        assert_eq!(loaded[0].tmdb_original_name, None);
    }

    #[test]
    fn test_upsert_preserves_tmdb_last_updated() {
        // Arrange
        let (conn, _dir) = setup_db();
        let titles = vec![make_title(100, "Test", "2024-01-01 00:00:00")];
        upsert_titles(&conn, &titles).unwrap();

        // Set tmdb_last_updated
        update_tmdb_last_updated(&conn, 100, "2026-02-19T12:00:00Z").unwrap();

        // Act: upsert with new last_update (tmdb_last_updated should be preserved)
        let updated = vec![make_title(100, "Updated", "2024-02-01 00:00:00")];
        upsert_titles(&conn, &updated).unwrap();
        let loaded = load_titles(&conn).unwrap();

        // Assert: tmdb_last_updated preserved
        assert_eq!(loaded[0].title, "Updated");
        assert_eq!(
            loaded[0].tmdb_last_updated.as_deref(),
            Some("2026-02-19T12:00:00Z")
        );
    }

    #[test]
    fn test_upsert_and_load_with_keywords() {
        // Arrange
        let (conn, _dir) = setup_db();
        let mut title = make_title(100, "Test", "2024-01-01 00:00:00");
        title.keywords = vec![
            String::from("spy"),
            String::from("family"),
            String::from("action"),
        ];

        // Act
        upsert_titles(&conn, &[title]).unwrap();
        let loaded = load_titles(&conn).unwrap();

        // Assert
        assert_eq!(loaded[0].keywords, vec!["spy", "family", "action"]);
    }

    #[test]
    fn test_upsert_and_load_empty_keywords() {
        // Arrange
        let (conn, _dir) = setup_db();
        let title = make_title(100, "Test", "2024-01-01 00:00:00");

        // Act
        upsert_titles(&conn, &[title]).unwrap();
        let loaded = load_titles(&conn).unwrap();

        // Assert
        assert!(loaded[0].keywords.is_empty());
    }

    #[test]
    fn test_filter_keywords_removes_wikipedia_prefix() {
        // Arrange
        let keywords = vec![String::from("anime"), String::from("wikipedia:Spy_Family")];

        // Act
        let filtered = filter_keywords(&keywords, "Test", None);

        // Assert
        assert_eq!(filtered, vec!["anime"]);
    }

    #[test]
    fn test_filter_keywords_removes_legwork_prefix() {
        // Arrange
        let keywords = vec![String::from("comedy"), String::from("legwork:some_value")];

        // Act
        let filtered = filter_keywords(&keywords, "Test", None);

        // Assert
        assert_eq!(filtered, vec!["comedy"]);
    }

    #[test]
    fn test_filter_keywords_removes_empty_and_whitespace() {
        // Arrange
        let keywords = vec![
            String::from("anime"),
            String::new(),
            String::from("  "),
            String::from("comedy"),
        ];

        // Act
        let filtered = filter_keywords(&keywords, "Test", None);

        // Assert
        assert_eq!(filtered, vec!["anime", "comedy"]);
    }

    #[test]
    fn test_filter_keywords_removes_title_match() {
        // Arrange
        let keywords = vec![
            String::from("SPY×FAMILY"),
            String::from("spy"),
            String::from("family"),
        ];

        // Act
        let filtered = filter_keywords(&keywords, "SPY×FAMILY", None);

        // Assert
        assert_eq!(filtered, vec!["spy", "family"]);
    }

    #[test]
    fn test_filter_keywords_removes_short_title_match() {
        // Arrange
        let keywords = vec![
            String::from("spy"),
            String::from("スパファミ"),
            String::from("family"),
        ];

        // Act
        let filtered = filter_keywords(&keywords, "SPY×FAMILY", Some("スパファミ"));

        // Assert
        assert_eq!(filtered, vec!["spy", "family"]);
    }

    #[test]
    fn test_filter_keywords_prefix_case_insensitive() {
        // Arrange
        let keywords = vec![
            String::from("anime"),
            String::from("Wikipedia:Test"),
            String::from("LEGWORK:value"),
            String::from("Wikiedia:Typo"),
        ];

        // Act
        let filtered = filter_keywords(&keywords, "Test Title", None);

        // Assert
        assert_eq!(filtered, vec!["anime"]);
    }
}
