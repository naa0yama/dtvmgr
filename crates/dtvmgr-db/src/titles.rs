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
    /// Keywords (nullable).
    pub keywords: Option<String>,
    /// Raw subtitle text (nullable).
    pub sub_titles: Option<String>,
    /// Last update timestamp.
    pub last_update: String,
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
                keywords, sub_titles, last_update
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
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
                t.keywords,
                t.sub_titles,
                t.last_update,
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
                    keywords, sub_titles, last_update
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
                keywords: row.get(11)?,
                sub_titles: row.get(12)?,
                last_update: row.get(13)?,
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
                keywords, sub_titles, last_update
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
                keywords: row.get(11)?,
                sub_titles: row.get(12)?,
                last_update: row.get(13)?,
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
            keywords: None,
            sub_titles: None,
            last_update: String::from(last_update),
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
}
