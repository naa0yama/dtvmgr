//! `EPGStation` recorded items cache CRUD operations.

use anyhow::{Context, Result};
use rusqlite::Connection;
use tracing::instrument;

/// A cached `EPGStation` recorded item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedRecordedItem {
    /// Recorded item ID.
    pub id: i64,
    /// Channel ID.
    pub channel_id: i64,
    /// Program name.
    pub name: String,
    /// Program description (nullable).
    pub description: Option<String>,
    /// Extended description (nullable).
    pub extended: Option<String>,
    /// Start timestamp (Unix ms).
    pub start_at: i64,
    /// End timestamp (Unix ms).
    pub end_at: i64,
    /// Whether currently recording.
    pub is_recording: bool,
    /// Whether currently encoding.
    pub is_encoding: bool,
    /// Whether protected from auto-delete.
    pub is_protected: bool,
    /// Video resolution (e.g. "1080i").
    pub video_resolution: Option<String>,
    /// Video type (e.g. "mpeg2").
    pub video_type: Option<String>,
    /// TS packet drop count.
    pub drop_cnt: i64,
    /// Error count.
    pub error_cnt: i64,
    /// Scrambling count.
    pub scrambling_cnt: i64,
    /// ISO 8601 timestamp of when this row was fetched.
    pub fetched_at: String,
}

/// A cached `EPGStation` video file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedVideoFile {
    /// Video file ID.
    pub id: i64,
    /// Parent recorded item ID.
    pub recorded_id: i64,
    /// Display name.
    pub name: String,
    /// Filename on disk (nullable).
    pub filename: Option<String>,
    /// File type ("ts" or "encoded").
    pub file_type: String,
    /// File size in bytes.
    pub size: i64,
    /// Whether the file exists on disk (nullable — unchecked).
    pub file_exists: Option<bool>,
    /// ISO 8601 timestamp of when file existence was last checked.
    pub file_checked_at: Option<String>,
}

/// Upserts recorded items and their video files. Returns rows changed.
///
/// # Errors
///
/// Returns an error if the database operation fails.
#[instrument(skip_all, err(level = "error"))]
pub fn upsert_recorded_items(
    conn: &Connection,
    items: &[CachedRecordedItem],
    video_files: &[(i64, Vec<CachedVideoFile>)],
) -> Result<usize> {
    let tx = conn
        .unchecked_transaction()
        .context("failed to begin transaction")?;

    let mut item_stmt = tx
        .prepare(
            "INSERT INTO epg_recorded_items (
                id, channel_id, name, description, extended,
                start_at, end_at, is_recording, is_encoding, is_protected,
                video_resolution, video_type,
                drop_cnt, error_cnt, scrambling_cnt, fetched_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            ON CONFLICT(id) DO UPDATE SET
                channel_id = excluded.channel_id,
                name = excluded.name,
                description = excluded.description,
                extended = excluded.extended,
                start_at = excluded.start_at,
                end_at = excluded.end_at,
                is_recording = excluded.is_recording,
                is_encoding = excluded.is_encoding,
                is_protected = excluded.is_protected,
                video_resolution = excluded.video_resolution,
                video_type = excluded.video_type,
                drop_cnt = excluded.drop_cnt,
                error_cnt = excluded.error_cnt,
                scrambling_cnt = excluded.scrambling_cnt,
                fetched_at = excluded.fetched_at",
        )
        .context("failed to prepare recorded items upsert")?;

    let mut vf_stmt = tx
        .prepare(
            "INSERT INTO epg_video_files (
                id, recorded_id, name, filename, file_type, size
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                recorded_id = excluded.recorded_id,
                name = excluded.name,
                filename = excluded.filename,
                file_type = excluded.file_type,
                size = excluded.size",
        )
        .context("failed to prepare video files upsert")?;

    let mut changed: usize = 0;
    for item in items {
        let rows = item_stmt
            .execute(rusqlite::params![
                item.id,
                item.channel_id,
                item.name,
                item.description,
                item.extended,
                item.start_at,
                item.end_at,
                item.is_recording,
                item.is_encoding,
                item.is_protected,
                item.video_resolution,
                item.video_type,
                item.drop_cnt,
                item.error_cnt,
                item.scrambling_cnt,
                item.fetched_at,
            ])
            .with_context(|| format!("failed to upsert recorded item {}", item.id))?;
        changed = changed.saturating_add(rows);
    }

    for (recorded_id, files) in video_files {
        for vf in files {
            vf_stmt
                .execute(rusqlite::params![
                    vf.id,
                    recorded_id,
                    vf.name,
                    vf.filename,
                    vf.file_type,
                    vf.size,
                ])
                .with_context(|| format!("failed to upsert video file {}", vf.id))?;
        }
    }

    drop(item_stmt);
    drop(vf_stmt);
    tx.commit()
        .context("failed to commit recorded items upsert")?;
    Ok(changed)
}

/// Loads a page of cached recorded items (newest first) with their video files.
///
/// Returns `(items_with_files, total_count)`.
///
/// # Errors
///
/// Returns an error if the database query fails.
#[allow(clippy::type_complexity)]
#[instrument(skip_all, err(level = "error"))]
pub fn load_recorded_items_page(
    conn: &Connection,
    offset: i64,
    limit: i64,
) -> Result<(Vec<(CachedRecordedItem, Vec<CachedVideoFile>)>, u64)> {
    // Get total count
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM epg_recorded_items", [], |row| {
            row.get(0)
        })
        .context("failed to count recorded items")?;

    let mut item_stmt = conn
        .prepare(
            "SELECT id, channel_id, name, description, extended,
                    start_at, end_at, is_recording, is_encoding, is_protected,
                    video_resolution, video_type,
                    drop_cnt, error_cnt, scrambling_cnt, fetched_at
             FROM epg_recorded_items
             ORDER BY start_at DESC
             LIMIT ?1 OFFSET ?2",
        )
        .context("failed to prepare recorded items query")?;

    let items: Vec<CachedRecordedItem> = item_stmt
        .query_map(rusqlite::params![limit, offset], map_recorded_row)
        .context("failed to query recorded items")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read recorded items rows")?;

    // Batch-load video files for all items in a single query to avoid N+1.
    let item_ids: Vec<i64> = items.iter().map(|i| i.id).collect();
    let all_files = load_video_files_for_items(conn, &item_ids)
        .context("failed to load video files for page")?;

    let mut result = Vec::with_capacity(items.len());
    for item in items {
        let files = all_files.get(&item.id).cloned().unwrap_or_default();
        result.push((item, files));
    }

    let total_u64 = u64::try_from(total.max(0)).unwrap_or(0);
    Ok((result, total_u64))
}

/// Loads all cached recorded items (newest first) with their video files.
///
/// # Errors
///
/// Returns an error if the database query fails.
#[instrument(skip_all, err(level = "error"))]
pub fn load_recorded_items(
    conn: &Connection,
) -> Result<Vec<(CachedRecordedItem, Vec<CachedVideoFile>)>> {
    let (items, _total) = load_recorded_items_page(conn, 0, i64::MAX)?;
    Ok(items)
}

/// Deletes recorded items whose ID is not in the given set. Returns the number of rows deleted.
///
/// # Errors
///
/// Returns an error if the database operation fails.
#[instrument(skip_all, err(level = "error"))]
pub fn delete_recorded_items_not_in(conn: &Connection, ids: &[i64]) -> Result<usize> {
    if ids.is_empty() {
        let deleted = conn
            .execute("DELETE FROM epg_recorded_items", [])
            .context("failed to delete all recorded items")?;
        return Ok(deleted);
    }

    // Use a temp table to avoid SQLite's 999-variable limit for large ID sets.
    conn.execute_batch("CREATE TEMP TABLE IF NOT EXISTS _keep_ids (id INTEGER PRIMARY KEY)")
        .context("failed to create temp table for delete filter")?;
    conn.execute("DELETE FROM _keep_ids", [])
        .context("failed to clear temp table")?;

    {
        let mut stmt = conn
            .prepare("INSERT OR IGNORE INTO _keep_ids (id) VALUES (?1)")
            .context("failed to prepare temp insert")?;
        for &id in ids {
            stmt.execute(rusqlite::params![id])
                .context("failed to insert into temp table")?;
        }
    }

    let deleted = conn
        .execute(
            "DELETE FROM epg_recorded_items WHERE id NOT IN (SELECT id FROM _keep_ids)",
            [],
        )
        .context("failed to delete recorded items by id filter")?;

    conn.execute_batch("DROP TABLE IF EXISTS _keep_ids")
        .context("failed to drop temp table")?;

    Ok(deleted)
}

/// Gets the newest `start_at` value in the cache (for incremental sync).
///
/// # Errors
///
/// Returns an error if the database query fails.
#[instrument(skip_all, err(level = "error"))]
pub fn newest_start_at(conn: &Connection) -> Result<Option<i64>> {
    conn.query_row("SELECT MAX(start_at) FROM epg_recorded_items", [], |row| {
        row.get(0)
    })
    .context("failed to get newest start_at")
}

/// Updates the file existence check result for a video file.
///
/// # Errors
///
/// Returns an error if the database operation fails.
#[instrument(skip_all, err(level = "error"))]
pub fn update_file_exists(
    conn: &Connection,
    video_file_id: i64,
    exists: bool,
    checked_at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE epg_video_files SET file_exists = ?1, file_checked_at = ?2 WHERE id = ?3",
        rusqlite::params![exists, checked_at, video_file_id],
    )
    .with_context(|| format!("failed to update file_exists for video file {video_file_id}"))?;
    Ok(())
}

/// Invalidates (clears) file existence for all video files of a recorded item.
///
/// # Errors
///
/// Returns an error if the database operation fails.
#[instrument(skip_all, err(level = "error"))]
pub fn invalidate_file_exists(conn: &Connection, recorded_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE epg_video_files SET file_exists = NULL, file_checked_at = NULL WHERE recorded_id = ?1",
        rusqlite::params![recorded_id],
    )
    .with_context(|| format!("failed to invalidate file_exists for recorded item {recorded_id}"))?;
    Ok(())
}

/// Batch-loads video files for a set of recorded item IDs.
///
/// Returns a map from `recorded_id` to the list of video files.
fn load_video_files_for_items(
    conn: &Connection,
    item_ids: &[i64],
) -> Result<std::collections::HashMap<i64, Vec<CachedVideoFile>>> {
    use std::collections::HashMap;

    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders: Vec<String> = item_ids.iter().map(|_| String::from("?")).collect();
    let sql = format!(
        "SELECT id, recorded_id, name, filename, file_type, size,
                file_exists, file_checked_at
         FROM epg_video_files
         WHERE recorded_id IN ({})
         ORDER BY recorded_id, id",
        placeholders.join(", ")
    );

    let params: Vec<Box<dyn rusqlite::types::ToSql>> = item_ids
        .iter()
        .map(|&id| -> Box<dyn rusqlite::types::ToSql> { Box::new(id) })
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(AsRef::as_ref).collect();

    let mut stmt = conn
        .prepare(&sql)
        .context("failed to prepare batch video files query")?;
    let files: Vec<CachedVideoFile> = stmt
        .query_map(param_refs.as_slice(), map_video_file_row)
        .context("failed to query batch video files")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read batch video file rows")?;

    let mut map: HashMap<i64, Vec<CachedVideoFile>> = HashMap::new();
    for vf in files {
        map.entry(vf.recorded_id).or_default().push(vf);
    }
    Ok(map)
}

/// Reads an INTEGER column as `bool` (0 = false, non-zero = true).
fn read_bool(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<bool> {
    let v: i32 = row.get(idx)?;
    Ok(v != 0)
}

/// Maps a database row to a `CachedRecordedItem`.
fn map_recorded_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CachedRecordedItem> {
    Ok(CachedRecordedItem {
        id: row.get(0)?,
        channel_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        extended: row.get(4)?,
        start_at: row.get(5)?,
        end_at: row.get(6)?,
        is_recording: read_bool(row, 7)?,
        is_encoding: read_bool(row, 8)?,
        is_protected: read_bool(row, 9)?,
        video_resolution: row.get(10)?,
        video_type: row.get(11)?,
        drop_cnt: row.get(12)?,
        error_cnt: row.get(13)?,
        scrambling_cnt: row.get(14)?,
        fetched_at: row.get(15)?,
    })
}

/// Maps a database row to a `CachedVideoFile`.
fn map_video_file_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CachedVideoFile> {
    let file_exists: Option<i32> = row.get(6)?;
    Ok(CachedVideoFile {
        id: row.get(0)?,
        recorded_id: row.get(1)?,
        name: row.get(2)?,
        filename: row.get(3)?,
        file_type: row.get(4)?,
        size: row.get(5)?,
        file_exists: file_exists.map(|v| v != 0),
        file_checked_at: row.get(7)?,
    })
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

    fn make_item(id: i64, start_at: i64) -> CachedRecordedItem {
        CachedRecordedItem {
            id,
            channel_id: 1,
            name: format!("Program {id}"),
            description: None,
            extended: None,
            start_at,
            end_at: start_at.saturating_add(1_800_000),
            is_recording: false,
            is_encoding: false,
            is_protected: false,
            video_resolution: Some(String::from("1080i")),
            video_type: Some(String::from("mpeg2")),
            drop_cnt: 0,
            error_cnt: 0,
            scrambling_cnt: 0,
            fetched_at: String::from("2024-01-01T00:00:00Z"),
        }
    }

    fn make_video_file(id: i64, recorded_id: i64, file_type: &str) -> CachedVideoFile {
        CachedVideoFile {
            id,
            recorded_id,
            name: format!("file_{id}"),
            filename: Some(format!("file_{id}.ts")),
            file_type: String::from(file_type),
            size: 1_000_000,
            file_exists: None,
            file_checked_at: None,
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_upsert_and_load_recorded_items() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items = vec![make_item(1, 1_000_000), make_item(2, 2_000_000)];
        let vf = vec![
            (1, vec![make_video_file(10, 1, "ts")]),
            (2, vec![make_video_file(20, 2, "ts")]),
        ];

        // Act
        let changed = upsert_recorded_items(&conn, &items, &vf).unwrap();
        let loaded = load_recorded_items(&conn).unwrap();

        // Assert
        assert_eq!(changed, 2);
        assert_eq!(loaded.len(), 2);
        // Newest first (start_at DESC)
        assert_eq!(loaded[0].0.id, 2);
        assert_eq!(loaded[1].0.id, 1);
        assert_eq!(loaded[0].1.len(), 1);
        assert_eq!(loaded[0].1[0].id, 20);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_upsert_updates_existing() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items = vec![make_item(1, 1_000_000)];
        let vf = vec![(1, vec![make_video_file(10, 1, "ts")])];
        upsert_recorded_items(&conn, &items, &vf).unwrap();

        // Act: update with new name
        let mut updated = make_item(1, 1_000_000);
        updated.name = String::from("Updated Program");
        upsert_recorded_items(&conn, &[updated], &vf).unwrap();
        let loaded = load_recorded_items(&conn).unwrap();

        // Assert
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0.name, "Updated Program");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_recorded_items_page() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items: Vec<CachedRecordedItem> = (1..=5).map(|i| make_item(i, i * 1_000_000)).collect();
        let vf: Vec<(i64, Vec<CachedVideoFile>)> = (1..=5)
            .map(|i| (i, vec![make_video_file(i * 10, i, "ts")]))
            .collect();
        upsert_recorded_items(&conn, &items, &vf).unwrap();

        // Act: get page 1 (2 items)
        let (page1, total) = load_recorded_items_page(&conn, 0, 2).unwrap();

        // Assert
        assert_eq!(total, 5);
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].0.id, 5); // newest first
        assert_eq!(page1[1].0.id, 4);

        // Act: get page 2
        let (page2, _) = load_recorded_items_page(&conn, 2, 2).unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].0.id, 3);
        assert_eq!(page2[1].0.id, 2);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_delete_recorded_items_not_in() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items = vec![make_item(1, 1_000_000), make_item(2, 2_000_000)];
        upsert_recorded_items(&conn, &items, &[]).unwrap();

        // Act: keep only item 1
        let deleted = delete_recorded_items_not_in(&conn, &[1]).unwrap();
        let remaining = load_recorded_items(&conn).unwrap();

        // Assert
        assert_eq!(deleted, 1);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].0.id, 1);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_delete_recorded_items_not_in_empty() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items = vec![make_item(1, 1_000_000)];
        upsert_recorded_items(&conn, &items, &[]).unwrap();

        // Act: empty list deletes all
        let deleted = delete_recorded_items_not_in(&conn, &[]).unwrap();
        let remaining = load_recorded_items(&conn).unwrap();

        // Assert
        assert_eq!(deleted, 1);
        assert!(remaining.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_newest_start_at() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items = vec![make_item(1, 1_000_000), make_item(2, 5_000_000)];
        upsert_recorded_items(&conn, &items, &[]).unwrap();

        // Act
        let newest = newest_start_at(&conn).unwrap();

        // Assert
        assert_eq!(newest, Some(5_000_000));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_newest_start_at_empty() {
        // Arrange
        let (conn, _dir) = setup_db();

        // Act
        let newest = newest_start_at(&conn).unwrap();

        // Assert
        assert_eq!(newest, None);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_update_file_exists() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items = vec![make_item(1, 1_000_000)];
        let vf = vec![(1, vec![make_video_file(10, 1, "ts")])];
        upsert_recorded_items(&conn, &items, &vf).unwrap();

        // Act
        update_file_exists(&conn, 10, true, "2024-01-01T12:00:00Z").unwrap();
        let loaded = load_recorded_items(&conn).unwrap();

        // Assert
        assert_eq!(loaded[0].1[0].file_exists, Some(true));
        assert_eq!(
            loaded[0].1[0].file_checked_at.as_deref(),
            Some("2024-01-01T12:00:00Z")
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_invalidate_file_exists() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items = vec![make_item(1, 1_000_000)];
        let vf = vec![(1, vec![make_video_file(10, 1, "ts")])];
        upsert_recorded_items(&conn, &items, &vf).unwrap();
        update_file_exists(&conn, 10, true, "2024-01-01T12:00:00Z").unwrap();

        // Act
        invalidate_file_exists(&conn, 1).unwrap();
        let loaded = load_recorded_items(&conn).unwrap();

        // Assert
        assert_eq!(loaded[0].1[0].file_exists, None);
        assert_eq!(loaded[0].1[0].file_checked_at, None);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_cascade_delete_video_files() {
        // Arrange
        let (conn, _dir) = setup_db();
        // Enable foreign keys for cascade
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        let items = vec![make_item(1, 1_000_000)];
        let vf = vec![(1, vec![make_video_file(10, 1, "ts")])];
        upsert_recorded_items(&conn, &items, &vf).unwrap();

        // Act: delete the recorded item
        conn.execute("DELETE FROM epg_recorded_items WHERE id = 1", [])
            .unwrap();

        // Assert: video files should be cascade-deleted
        let count: u64 = conn
            .query_row("SELECT COUNT(*) FROM epg_video_files", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_upsert_empty_items() {
        // Arrange
        let (conn, _dir) = setup_db();

        // Act
        let changed = upsert_recorded_items(&conn, &[], &[]).unwrap();

        // Assert
        assert_eq!(changed, 0);
        let loaded = load_recorded_items(&conn).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_recorded_items_page_empty() {
        // Arrange
        let (conn, _dir) = setup_db();

        // Act
        let (items, total) = load_recorded_items_page(&conn, 0, 10).unwrap();

        // Assert
        assert_eq!(total, 0);
        assert!(items.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_recorded_items_page_offset_beyond() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items = vec![make_item(1, 1_000_000)];
        upsert_recorded_items(&conn, &items, &[]).unwrap();

        // Act — offset beyond the total count
        let (page, total) = load_recorded_items_page(&conn, 100, 10).unwrap();

        // Assert
        assert_eq!(total, 1);
        assert!(page.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_update_file_exists_false() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items = vec![make_item(1, 1_000_000)];
        let vf = vec![(1, vec![make_video_file(10, 1, "ts")])];
        upsert_recorded_items(&conn, &items, &vf).unwrap();

        // Act — mark as not existing
        update_file_exists(&conn, 10, false, "2024-06-01T00:00:00Z").unwrap();
        let loaded = load_recorded_items(&conn).unwrap();

        // Assert
        assert_eq!(loaded[0].1[0].file_exists, Some(false));
        assert_eq!(
            loaded[0].1[0].file_checked_at.as_deref(),
            Some("2024-06-01T00:00:00Z")
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_delete_recorded_items_not_in_all_kept() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items = vec![make_item(1, 1_000_000), make_item(2, 2_000_000)];
        upsert_recorded_items(&conn, &items, &[]).unwrap();

        // Act — keep all items
        let deleted = delete_recorded_items_not_in(&conn, &[1, 2]).unwrap();
        let remaining = load_recorded_items(&conn).unwrap();

        // Assert
        assert_eq!(deleted, 0);
        assert_eq!(remaining.len(), 2);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_upsert_multiple_video_files() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items = vec![make_item(1, 1_000_000)];
        let vf = vec![(
            1,
            vec![
                make_video_file(10, 1, "ts"),
                make_video_file(11, 1, "encoded"),
            ],
        )];

        // Act
        upsert_recorded_items(&conn, &items, &vf).unwrap();
        let loaded = load_recorded_items(&conn).unwrap();

        // Assert
        assert_eq!(loaded[0].1.len(), 2);
        assert_eq!(loaded[0].1[0].file_type, "ts");
        assert_eq!(loaded[0].1[1].file_type, "encoded");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_invalidate_file_exists_no_video_files() {
        // Arrange
        let (conn, _dir) = setup_db();
        let items = vec![make_item(1, 1_000_000)];
        upsert_recorded_items(&conn, &items, &[]).unwrap();

        // Act — should not error even when no video files exist
        invalidate_file_exists(&conn, 1).unwrap();
    }
}
