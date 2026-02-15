//! Schema version management using `PRAGMA user_version`.

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Current schema version.
const CURRENT_VERSION: u32 = 1;

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
    }
}
