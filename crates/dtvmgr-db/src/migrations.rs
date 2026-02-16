//! Schema version management using `PRAGMA user_version`.

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Current schema version.
const CURRENT_VERSION: u32 = 3;

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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
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
    fn test_v2_to_v3_migration() {
        // Arrange: start from v2
        let conn = Connection::open_in_memory().unwrap();
        migrate_v1(&conn).unwrap();
        migrate_v2(&conn).unwrap();
        conn.pragma_update(None, "user_version", 2u32).unwrap();

        // Act: run full migrations (should apply v3)
        run_migrations(&conn).unwrap();

        // Assert
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);

        // Verify duration_min column exists
        let stmt = conn
            .prepare("SELECT duration_min FROM programs LIMIT 0")
            .unwrap();
        let col_count = stmt.column_count();
        assert_eq!(col_count, 1);
    }
}
