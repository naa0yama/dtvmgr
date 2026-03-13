//! Schema version management using `PRAGMA user_version`.

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Current schema version.
const CURRENT_VERSION: u32 = 7;

/// Runs database migrations up to `CURRENT_VERSION`.
///
/// # Errors
///
/// Returns an error if any SQL statement fails.
pub fn run_migrations(conn: &Connection) -> Result<()> {
    let version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .context("failed to read user_version")?;

    if version < 1 {
        migrate_v1(conn).context("migration to v1 failed")?;
    }
    if version < 2 {
        migrate_v2(conn).context("migration to v2 failed")?;
    }
    if version < 3 {
        migrate_v3(conn).context("migration to v3 failed")?;
    }
    if version < 4 {
        migrate_v4(conn).context("migration to v4 failed")?;
    }
    if version < 5 {
        migrate_v5(conn).context("migration to v5 failed")?;
    }
    if version < 6 {
        migrate_v6(conn).context("migration to v6 failed")?;
    }
    if version < 7 {
        migrate_v7(conn).context("migration to v7 failed")?;
    }

    conn.pragma_update(None, "user_version", CURRENT_VERSION)
        .context("failed to update user_version")?;

    Ok(())
}

/// Migration to v1: create `channel_groups` and `channels` tables.
fn migrate_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS channel_groups (
            ch_gid          INTEGER PRIMARY KEY,
            ch_group_name   TEXT NOT NULL,
            ch_group_order  INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS channels (
            ch_id    INTEGER PRIMARY KEY,
            ch_gid   INTEGER REFERENCES channel_groups(ch_gid),
            ch_name  TEXT NOT NULL
        );",
    )
    .context("failed to create tables")?;

    Ok(())
}

/// Migration to v2: create `titles` and `programs` tables.
fn migrate_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS titles (
            tid                 INTEGER PRIMARY KEY,
            tmdb_series_id      INTEGER,
            tmdb_season_number  INTEGER,
            title               TEXT NOT NULL,
            short_title         TEXT,
            title_yomi          TEXT,
            title_en            TEXT,
            cat                 INTEGER,
            title_flag          INTEGER,
            first_year          INTEGER,
            first_month         INTEGER,
            keywords            TEXT,
            sub_titles          TEXT,
            last_update         TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS programs (
            pid              INTEGER PRIMARY KEY,
            tid              INTEGER NOT NULL REFERENCES titles(tid),
            ch_id            INTEGER NOT NULL REFERENCES channels(ch_id),
            tmdb_episode_id  INTEGER,
            st_time          TEXT NOT NULL,
            st_offset        INTEGER,
            ed_time          TEXT NOT NULL,
            count            INTEGER,
            sub_title        TEXT,
            flag             INTEGER,
            deleted          INTEGER,
            warn             INTEGER,
            revision         INTEGER,
            last_update      TEXT,
            st_sub_title     TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_programs_tid ON programs(tid);
        CREATE INDEX IF NOT EXISTS idx_programs_ch_id ON programs(ch_id);
        CREATE INDEX IF NOT EXISTS idx_programs_st_time ON programs(st_time);
        CREATE INDEX IF NOT EXISTS idx_titles_tmdb_series_id ON titles(tmdb_series_id);",
    )
    .context("failed to create titles/programs tables")?;

    Ok(())
}

/// Migration to v3: add `duration_min` column to `programs` and backfill.
fn migrate_v3(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "ALTER TABLE programs ADD COLUMN duration_min INTEGER;
         UPDATE programs SET duration_min = CAST(ROUND((julianday(ed_time) - julianday(st_time)) * 24 * 60) AS INTEGER);",
    )
    .context("failed to add duration_min column")?;

    Ok(())
}

/// Migration to v4: add TMDB search result columns to `titles`.
fn migrate_v4(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "ALTER TABLE titles ADD COLUMN tmdb_original_name TEXT;
         ALTER TABLE titles ADD COLUMN tmdb_name TEXT;
         ALTER TABLE titles ADD COLUMN tmdb_alt_titles TEXT;",
    )
    .context("failed to add TMDB search result columns")?;

    Ok(())
}

/// Migration to v5: add `tmdb_last_updated` timestamp column to `titles`.
fn migrate_v5(conn: &Connection) -> Result<()> {
    conn.execute_batch("ALTER TABLE titles ADD COLUMN tmdb_last_updated TEXT;")
        .context("failed to add tmdb_last_updated column")?;

    Ok(())
}

/// Migration to v6: add `tmdb_season_id` column to `titles`.
fn migrate_v6(conn: &Connection) -> Result<()> {
    conn.execute_batch("ALTER TABLE titles ADD COLUMN tmdb_season_id INTEGER;")
        .context("failed to add tmdb_season_id column")?;

    Ok(())
}

/// Migration to v7: create `EPGStation` recorded items and video files cache tables.
fn migrate_v7(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS epg_recorded_items (
            id              INTEGER PRIMARY KEY,
            channel_id      INTEGER NOT NULL,
            name            TEXT NOT NULL,
            description     TEXT,
            extended        TEXT,
            start_at        INTEGER NOT NULL,
            end_at          INTEGER NOT NULL,
            is_recording    INTEGER NOT NULL DEFAULT 0,
            is_encoding     INTEGER NOT NULL DEFAULT 0,
            is_protected    INTEGER NOT NULL DEFAULT 0,
            video_resolution TEXT,
            video_type      TEXT,
            drop_cnt        INTEGER NOT NULL DEFAULT 0,
            error_cnt       INTEGER NOT NULL DEFAULT 0,
            scrambling_cnt  INTEGER NOT NULL DEFAULT 0,
            fetched_at      TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS epg_video_files (
            id              INTEGER PRIMARY KEY,
            recorded_id     INTEGER NOT NULL REFERENCES epg_recorded_items(id) ON DELETE CASCADE,
            name            TEXT NOT NULL,
            filename        TEXT,
            file_type       TEXT NOT NULL,
            size            INTEGER NOT NULL DEFAULT 0,
            file_exists     INTEGER,
            file_checked_at TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_epg_recorded_start_at ON epg_recorded_items(start_at DESC);
        CREATE INDEX IF NOT EXISTS idx_epg_video_files_recorded_id ON epg_video_files(recorded_id);",
    )
    .context("failed to create EPGStation cache tables")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_migrations_idempotent() {
        // Arrange
        let conn = Connection::open_in_memory().unwrap();

        // Act
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        // Assert
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_tables_exist_after_migration() {
        // Arrange
        let conn = Connection::open_in_memory().unwrap();

        // Act
        run_migrations(&conn).unwrap();

        // Assert
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(tables.contains(&String::from("channel_groups")));
        assert!(tables.contains(&String::from("channels")));
        assert!(tables.contains(&String::from("titles")));
        assert!(tables.contains(&String::from("programs")));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_v1_to_v2_migration() {
        // Arrange: start from v1
        let conn = Connection::open_in_memory().unwrap();
        migrate_v1(&conn).unwrap();
        conn.pragma_update(None, "user_version", 1u32).unwrap();

        // Act: run full migrations (should apply v2)
        run_migrations(&conn).unwrap();

        // Assert
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);

        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(tables.contains(&String::from("titles")));
        assert!(tables.contains(&String::from("programs")));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_v2_to_v3_migration() {
        // Arrange: start from v2
        let conn = Connection::open_in_memory().unwrap();
        migrate_v1(&conn).unwrap();
        migrate_v2(&conn).unwrap();
        conn.pragma_update(None, "user_version", 2u32).unwrap();

        // Act: run full migrations (should apply v3+v4)
        run_migrations(&conn).unwrap();

        // Assert
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);

        // Verify duration_min column exists
        let stmt = conn
            .prepare("SELECT duration_min FROM programs LIMIT 0")
            .unwrap();
        let col_count = stmt.column_count();
        assert_eq!(col_count, 1);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_v3_to_v4_migration() {
        // Arrange: start from v3
        let conn = Connection::open_in_memory().unwrap();
        migrate_v1(&conn).unwrap();
        migrate_v2(&conn).unwrap();
        migrate_v3(&conn).unwrap();
        conn.pragma_update(None, "user_version", 3u32).unwrap();

        // Act: run full migrations (should apply v4)
        run_migrations(&conn).unwrap();

        // Assert
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);

        // Verify new columns exist
        let stmt = conn
            .prepare("SELECT tmdb_original_name, tmdb_name, tmdb_alt_titles FROM titles LIMIT 0")
            .unwrap();
        assert_eq!(stmt.column_count(), 3);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_v4_to_v5_migration() {
        // Arrange: start from v4
        let conn = Connection::open_in_memory().unwrap();
        migrate_v1(&conn).unwrap();
        migrate_v2(&conn).unwrap();
        migrate_v3(&conn).unwrap();
        migrate_v4(&conn).unwrap();
        conn.pragma_update(None, "user_version", 4u32).unwrap();

        // Act: run full migrations (should apply v5)
        run_migrations(&conn).unwrap();

        // Assert
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);

        // Verify tmdb_last_updated column exists
        let stmt = conn
            .prepare("SELECT tmdb_last_updated FROM titles LIMIT 0")
            .unwrap();
        assert_eq!(stmt.column_count(), 1);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_v6_to_v7_migration() {
        // Arrange: start from v6
        let conn = Connection::open_in_memory().unwrap();
        migrate_v1(&conn).unwrap();
        migrate_v2(&conn).unwrap();
        migrate_v3(&conn).unwrap();
        migrate_v4(&conn).unwrap();
        migrate_v5(&conn).unwrap();
        migrate_v6(&conn).unwrap();
        conn.pragma_update(None, "user_version", 6u32).unwrap();

        // Act: run full migrations (should apply v7)
        run_migrations(&conn).unwrap();

        // Assert
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);

        // Verify tables exist
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(tables.contains(&String::from("epg_recorded_items")));
        assert!(tables.contains(&String::from("epg_video_files")));

        // Verify columns exist
        let stmt = conn
            .prepare("SELECT id, channel_id, name, fetched_at FROM epg_recorded_items LIMIT 0")
            .unwrap();
        assert_eq!(stmt.column_count(), 4);

        let stmt = conn
            .prepare("SELECT id, recorded_id, file_type, file_exists, file_checked_at FROM epg_video_files LIMIT 0")
            .unwrap();
        assert_eq!(stmt.column_count(), 5);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_v5_to_v6_migration() {
        // Arrange: start from v5
        let conn = Connection::open_in_memory().unwrap();
        migrate_v1(&conn).unwrap();
        migrate_v2(&conn).unwrap();
        migrate_v3(&conn).unwrap();
        migrate_v4(&conn).unwrap();
        migrate_v5(&conn).unwrap();
        conn.pragma_update(None, "user_version", 5u32).unwrap();

        // Act: run full migrations (should apply v6)
        run_migrations(&conn).unwrap();

        // Assert
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);

        // Verify tmdb_season_id column exists
        let stmt = conn
            .prepare("SELECT tmdb_season_id FROM titles LIMIT 0")
            .unwrap();
        assert_eq!(stmt.column_count(), 1);
    }
}
