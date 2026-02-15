//! Database connection management.

use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::Connection;

use super::migrations::run_migrations;

/// Opens (or creates) the database and runs migrations.
///
/// - If `dir` is `Some`, uses `{dir}/dtvmgr.db`.
/// - Otherwise uses `~/.local/share/dtvmgr/dtvmgr.db`.
///
/// # Errors
///
/// Returns an error if the database cannot be opened or migrations fail.
pub fn open_db(dir: Option<&PathBuf>) -> Result<Connection> {
    let db_path = resolve_db_path(dir)?;

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let conn = Connection::open(&db_path)
        .with_context(|| format!("failed to open database {}", db_path.display()))?;

    run_migrations(&conn).context("database migration failed")?;

    Ok(conn)
}

/// Resolves the database file path.
fn resolve_db_path(dir: Option<&PathBuf>) -> Result<PathBuf> {
    if let Some(d) = dir {
        return Ok(d.join("dtvmgr.db"));
    }

    let home = std::env::var("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("dtvmgr")
        .join("dtvmgr.db"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn test_open_db_in_temp_dir() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        // Act
        let conn = open_db(Some(&dir_path)).unwrap();

        // Assert
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert!(version > 0);
    }

    #[test]
    fn test_resolve_db_path_with_dir() {
        // Arrange
        let dir = PathBuf::from("/tmp/myproject");

        // Act
        let path = resolve_db_path(Some(&dir)).unwrap();

        // Assert
        assert_eq!(path, PathBuf::from("/tmp/myproject/dtvmgr.db"));
    }

    #[test]
    fn test_resolve_db_path_default() {
        // Arrange & Act
        let path = resolve_db_path(None).unwrap();

        // Assert
        assert!(path.ends_with(".local/share/dtvmgr/dtvmgr.db"));
    }
}
