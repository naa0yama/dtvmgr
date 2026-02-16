//! Program cache CRUD operations.

use anyhow::{Context, Result};
use rusqlite::Connection;

/// A cached program with optional TMDB mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedProgram {
    /// Syoboi program ID.
    pub pid: u32,
    /// Syoboi title ID (FK → titles.tid).
    pub tid: u32,
    /// Syoboi channel ID (FK → `channels.ch_id`).
    pub ch_id: u32,
    /// Mapped TMDB episode ID (cache, nullable).
    pub tmdb_episode_id: Option<u64>,
    /// Broadcast start time.
    pub st_time: String,
    /// Start offset in seconds (nullable).
    pub st_offset: Option<i32>,
    /// Broadcast end time.
    pub ed_time: String,
    /// Episode number (nullable).
    pub count: Option<u32>,
    /// Subtitle (nullable).
    pub sub_title: Option<String>,
    /// Flag bitmask (nullable).
    pub flag: Option<u32>,
    /// Deleted flag (nullable).
    pub deleted: Option<u32>,
    /// Warning flag (nullable).
    pub warn: Option<u32>,
    /// Revision number (nullable).
    pub revision: Option<u32>,
    /// Last update timestamp (nullable).
    pub last_update: Option<String>,
    /// Subtitle from `SubTitles` JOIN (nullable).
    pub st_sub_title: Option<String>,
    /// Duration in minutes (`ed_time` - `st_time`).
    pub duration_min: Option<u32>,
}

/// Upserts programs into the cache. Returns the number of rows changed.
///
/// Uses `INSERT ... ON CONFLICT(pid) DO UPDATE SET` to update existing rows.
/// The `tmdb_episode_id` column is preserved on conflict to avoid
/// overwriting manual TMDB mappings.
/// Only updates when `last_update` has changed.
///
/// # Errors
///
/// Returns an error if the database operation fails.
#[allow(clippy::module_name_repetitions)]
pub fn upsert_programs(conn: &Connection, programs: &[CachedProgram]) -> Result<usize> {
    let tx = conn
        .unchecked_transaction()
        .context("failed to begin transaction")?;

    let mut stmt = tx
        .prepare(
            "INSERT INTO programs (
                pid, tid, ch_id, tmdb_episode_id,
                st_time, st_offset, ed_time, count,
                sub_title, flag, deleted, warn,
                revision, last_update, st_sub_title, duration_min
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                CAST(ROUND((julianday(?7) - julianday(?5)) * 24 * 60) AS INTEGER))
            ON CONFLICT(pid) DO UPDATE SET
                tid = excluded.tid,
                ch_id = excluded.ch_id,
                st_time = excluded.st_time,
                st_offset = excluded.st_offset,
                ed_time = excluded.ed_time,
                count = excluded.count,
                sub_title = excluded.sub_title,
                flag = excluded.flag,
                deleted = excluded.deleted,
                warn = excluded.warn,
                revision = excluded.revision,
                last_update = excluded.last_update,
                st_sub_title = excluded.st_sub_title,
                duration_min = CAST(ROUND((julianday(excluded.ed_time) - julianday(excluded.st_time)) * 24 * 60) AS INTEGER)
            WHERE programs.last_update IS NOT excluded.last_update",
        )
        .context("failed to prepare programs upsert")?;

    let mut changed: usize = 0;
    for p in programs {
        let rows = stmt
            .execute(rusqlite::params![
                p.pid,
                p.tid,
                p.ch_id,
                p.tmdb_episode_id,
                p.st_time,
                p.st_offset,
                p.ed_time,
                p.count,
                p.sub_title,
                p.flag,
                p.deleted,
                p.warn,
                p.revision,
                p.last_update,
                p.st_sub_title,
            ])
            .with_context(|| format!("failed to upsert program {}", p.pid))?;
        changed = changed.saturating_add(rows);
    }

    drop(stmt);
    tx.commit().context("failed to commit programs upsert")?;
    Ok(changed)
}

/// Loads all programs from the cache, ordered by `st_time`.
///
/// # Errors
///
/// Returns an error if the database query fails.
#[allow(clippy::module_name_repetitions)]
pub fn load_programs(conn: &Connection) -> Result<Vec<CachedProgram>> {
    let mut stmt = conn
        .prepare(
            "SELECT pid, tid, ch_id, tmdb_episode_id,
                    st_time, st_offset, ed_time, count,
                    sub_title, flag, deleted, warn,
                    revision, last_update, st_sub_title, duration_min
             FROM programs
             ORDER BY st_time",
        )
        .context("failed to prepare programs query")?;

    let rows = stmt
        .query_map([], map_program_row)
        .context("failed to query programs")?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read programs rows")
}

/// Loads programs filtered by title IDs.
///
/// # Errors
///
/// Returns an error if the database query fails.
#[allow(clippy::module_name_repetitions)]
pub fn load_programs_by_tids(conn: &Connection, tids: &[u32]) -> Result<Vec<CachedProgram>> {
    if tids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = tids.iter().map(|_| String::from("?")).collect();
    let sql = format!(
        "SELECT pid, tid, ch_id, tmdb_episode_id,
                st_time, st_offset, ed_time, count,
                sub_title, flag, deleted, warn,
                revision, last_update, st_sub_title, duration_min
         FROM programs
         WHERE tid IN ({})
         ORDER BY st_time",
        placeholders.join(", ")
    );

    let mut stmt = conn
        .prepare(&sql)
        .context("failed to prepare programs query")?;

    let params: Vec<Box<dyn rusqlite::types::ToSql>> = tids
        .iter()
        .map(|tid| -> Box<dyn rusqlite::types::ToSql> { Box::new(*tid) })
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(AsRef::as_ref).collect();

    let rows = stmt
        .query_map(param_refs.as_slice(), map_program_row)
        .context("failed to query programs by tids")?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read programs rows")
}

/// Maps a database row to a `CachedProgram`.
fn map_program_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CachedProgram> {
    Ok(CachedProgram {
        pid: row.get(0)?,
        tid: row.get(1)?,
        ch_id: row.get(2)?,
        tmdb_episode_id: row.get(3)?,
        st_time: row.get(4)?,
        st_offset: row.get(5)?,
        ed_time: row.get(6)?,
        count: row.get(7)?,
        sub_title: row.get(8)?,
        flag: row.get(9)?,
        deleted: row.get(10)?,
        warn: row.get(11)?,
        revision: row.get(12)?,
        last_update: row.get(13)?,
        st_sub_title: row.get(14)?,
        duration_min: row.get(15)?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use crate::connection::open_db;
    use crate::titles::{CachedTitle, upsert_titles};

    fn setup_db() -> (Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(Some(&dir.path().to_path_buf())).unwrap();

        // Insert prerequisite channel and title data for FK constraints
        conn.execute(
            "INSERT INTO channel_groups (ch_gid, ch_group_name, ch_group_order) VALUES (1, 'Test', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO channels (ch_id, ch_gid, ch_name) VALUES (1, 1, 'TestCh')",
            [],
        )
        .unwrap();

        let titles = vec![CachedTitle {
            tid: 100,
            tmdb_series_id: None,
            tmdb_season_number: None,
            title: String::from("Test Title"),
            short_title: None,
            title_yomi: None,
            title_en: None,
            cat: None,
            title_flag: None,
            first_year: None,
            first_month: None,
            keywords: None,
            sub_titles: None,
            last_update: String::from("2024-01-01 00:00:00"),
        }];
        upsert_titles(&conn, &titles).unwrap();

        (conn, dir)
    }

    fn make_program(pid: u32, st_time: &str) -> CachedProgram {
        // Derive ed_time = st_time + 30 minutes
        let date = &st_time[..10];
        let hh: u32 = st_time[11..13].parse().unwrap();
        let mm: u32 = st_time[14..16].parse().unwrap();
        #[allow(clippy::arithmetic_side_effects)]
        let total = hh * 60 + mm + 30;
        let ed_time = format!("{date} {:02}:{:02}:00", total / 60, total % 60);
        CachedProgram {
            pid,
            tid: 100,
            ch_id: 1,
            tmdb_episode_id: None,
            st_time: String::from(st_time),
            st_offset: None,
            ed_time,
            count: Some(1),
            sub_title: Some(String::from("Episode 1")),
            flag: None,
            deleted: None,
            warn: None,
            revision: None,
            last_update: Some(String::from("2024-01-01 00:00:00")),
            st_sub_title: None,
            duration_min: None,
        }
    }

    #[test]
    fn test_upsert_and_load_programs() {
        // Arrange
        let (conn, _dir) = setup_db();
        let programs = vec![
            make_program(1, "2024-01-01 00:00:00"),
            make_program(2, "2024-01-01 01:00:00"),
        ];

        // Act
        let changed = upsert_programs(&conn, &programs).unwrap();
        let loaded = load_programs(&conn).unwrap();

        // Assert
        assert_eq!(changed, 2);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].pid, 1);
        assert_eq!(loaded[1].pid, 2);
    }

    #[test]
    fn test_upsert_programs_updates_existing() {
        // Arrange
        let (conn, _dir) = setup_db();
        let programs = vec![make_program(1, "2024-01-01 00:00:00")];
        upsert_programs(&conn, &programs).unwrap();

        // Act: upsert with updated last_update (triggers update)
        let mut updated = make_program(1, "2024-01-01 00:00:00");
        updated.sub_title = Some(String::from("Updated Episode"));
        updated.last_update = Some(String::from("2024-02-01 00:00:00"));
        let changed = upsert_programs(&conn, &[updated]).unwrap();
        let loaded = load_programs(&conn).unwrap();

        // Assert
        assert_eq!(changed, 1);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].sub_title.as_deref(), Some("Updated Episode"));
    }

    #[test]
    fn test_upsert_programs_skips_when_last_update_unchanged() {
        // Arrange
        let (conn, _dir) = setup_db();
        let programs = vec![make_program(1, "2024-01-01 00:00:00")];
        upsert_programs(&conn, &programs).unwrap();

        // Act: upsert with same last_update
        let same = make_program(1, "2024-01-01 00:00:00");
        let changed = upsert_programs(&conn, &[same]).unwrap();
        let loaded = load_programs(&conn).unwrap();

        // Assert: 0 rows changed
        assert_eq!(changed, 0);
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn test_upsert_preserves_tmdb_episode_id() {
        // Arrange
        let (conn, _dir) = setup_db();
        let mut prog = make_program(1, "2024-01-01 00:00:00");
        prog.tmdb_episode_id = Some(99999);
        upsert_programs(&conn, &[prog]).unwrap();

        // Act: upsert without tmdb_episode_id but with new last_update
        let mut updated = make_program(1, "2024-01-01 00:00:00");
        updated.last_update = Some(String::from("2024-02-01 00:00:00"));
        upsert_programs(&conn, &[updated]).unwrap();
        let loaded = load_programs(&conn).unwrap();

        // Assert: TMDB mapping preserved
        assert_eq!(loaded[0].tmdb_episode_id, Some(99999));
    }

    #[test]
    fn test_load_programs_by_tids() {
        // Arrange
        let (conn, _dir) = setup_db();

        // Add a second title
        let title2 = CachedTitle {
            tid: 200,
            tmdb_series_id: None,
            tmdb_season_number: None,
            title: String::from("Title 2"),
            short_title: None,
            title_yomi: None,
            title_en: None,
            cat: None,
            title_flag: None,
            first_year: None,
            first_month: None,
            keywords: None,
            sub_titles: None,
            last_update: String::from("2024-01-01 00:00:00"),
        };
        upsert_titles(&conn, &[title2]).unwrap();

        let mut prog1 = make_program(1, "2024-01-01 00:00:00");
        prog1.tid = 100;
        let mut prog2 = make_program(2, "2024-01-01 01:00:00");
        prog2.tid = 200;
        let mut prog3 = make_program(3, "2024-01-01 02:00:00");
        prog3.tid = 100;
        upsert_programs(&conn, &[prog1, prog2, prog3]).unwrap();

        // Act
        let loaded = load_programs_by_tids(&conn, &[100]).unwrap();

        // Assert
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().all(|p| p.tid == 100));
    }

    #[test]
    fn test_load_programs_by_tids_empty() {
        // Arrange
        let (conn, _dir) = setup_db();

        // Act
        let loaded = load_programs_by_tids(&conn, &[]).unwrap();

        // Assert
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_upsert_computes_duration_min() {
        // Arrange
        let (conn, _dir) = setup_db();
        let mut prog = make_program(1, "2024-01-01 00:00:00");
        prog.ed_time = String::from("2024-01-01 00:30:00");

        // Act
        upsert_programs(&conn, &[prog]).unwrap();
        let loaded = load_programs(&conn).unwrap();

        // Assert: 30 minutes duration
        assert_eq!(loaded[0].duration_min, Some(30));
    }

    #[test]
    fn test_load_programs_ordered_by_st_time() {
        // Arrange
        let (conn, _dir) = setup_db();
        let programs = vec![
            make_program(2, "2024-01-02 00:00:00"),
            make_program(1, "2024-01-01 00:00:00"),
            make_program(3, "2024-01-03 00:00:00"),
        ];
        upsert_programs(&conn, &programs).unwrap();

        // Act
        let loaded = load_programs(&conn).unwrap();

        // Assert: ordered by st_time
        assert_eq!(loaded[0].pid, 1);
        assert_eq!(loaded[1].pid, 2);
        assert_eq!(loaded[2].pid, 3);
    }
}
