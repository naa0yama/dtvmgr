//! Channel and channel group cache CRUD operations.

use anyhow::{Context, Result};
use rusqlite::Connection;

/// A cached channel group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedChannelGroup {
    /// Channel group ID.
    pub ch_gid: u32,
    /// Group display name.
    pub ch_group_name: String,
    /// Display order for sorting.
    pub ch_group_order: u32,
}

/// A cached channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedChannel {
    /// Channel ID.
    pub ch_id: u32,
    /// Channel group ID.
    pub ch_gid: Option<u32>,
    /// Channel name.
    pub ch_name: String,
}

/// Replaces all channel groups in the cache.
///
/// # Errors
///
/// Returns an error if the database operation fails.
pub fn save_channel_groups(conn: &Connection, groups: &[CachedChannelGroup]) -> Result<()> {
    let tx = conn
        .unchecked_transaction()
        .context("failed to begin transaction")?;

    // Delete channels first to satisfy FK constraint (channels.ch_gid -> channel_groups.ch_gid)
    tx.execute("DELETE FROM channels", [])
        .context("failed to clear channels (FK dependency)")?;

    tx.execute("DELETE FROM channel_groups", [])
        .context("failed to clear channel_groups")?;

    let mut stmt = tx
        .prepare("INSERT INTO channel_groups (ch_gid, ch_group_name, ch_group_order) VALUES (?1, ?2, ?3)")
        .context("failed to prepare channel_groups insert")?;

    for g in groups {
        stmt.execute(rusqlite::params![
            g.ch_gid,
            g.ch_group_name,
            g.ch_group_order
        ])
        .with_context(|| format!("failed to insert channel_group {}", g.ch_gid))?;
    }

    drop(stmt);
    tx.commit().context("failed to commit channel_groups")?;
    Ok(())
}

/// Loads all channel groups from the cache, ordered by `ch_group_order`.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub fn load_channel_groups(conn: &Connection) -> Result<Vec<CachedChannelGroup>> {
    let mut stmt = conn
        .prepare("SELECT ch_gid, ch_group_name, ch_group_order FROM channel_groups ORDER BY ch_group_order")
        .context("failed to prepare channel_groups query")?;

    let rows = stmt
        .query_map([], |row| {
            Ok(CachedChannelGroup {
                ch_gid: row.get(0)?,
                ch_group_name: row.get(1)?,
                ch_group_order: row.get(2)?,
            })
        })
        .context("failed to query channel_groups")?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read channel_groups rows")
}

/// Replaces all channels in the cache.
///
/// # Errors
///
/// Returns an error if the database operation fails.
#[allow(clippy::module_name_repetitions)]
pub fn save_channels(conn: &Connection, channels: &[CachedChannel]) -> Result<()> {
    let tx = conn
        .unchecked_transaction()
        .context("failed to begin transaction")?;

    tx.execute("DELETE FROM channels", [])
        .context("failed to clear channels")?;

    let mut stmt = tx
        .prepare("INSERT INTO channels (ch_id, ch_gid, ch_name) VALUES (?1, ?2, ?3)")
        .context("failed to prepare channels insert")?;

    for ch in channels {
        stmt.execute(rusqlite::params![ch.ch_id, ch.ch_gid, ch.ch_name])
            .with_context(|| format!("failed to insert channel {}", ch.ch_id))?;
    }

    drop(stmt);
    tx.commit().context("failed to commit channels")?;
    Ok(())
}

/// Loads all channels from the cache.
///
/// # Errors
///
/// Returns an error if the database query fails.
#[allow(clippy::module_name_repetitions)]
pub fn load_channels(conn: &Connection) -> Result<Vec<CachedChannel>> {
    let mut stmt = conn
        .prepare("SELECT ch_id, ch_gid, ch_name FROM channels ORDER BY ch_id")
        .context("failed to prepare channels query")?;

    let rows = stmt
        .query_map([], |row| {
            Ok(CachedChannel {
                ch_id: row.get(0)?,
                ch_gid: row.get(1)?,
                ch_name: row.get(2)?,
            })
        })
        .context("failed to query channels")?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read channels rows")
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

    #[test]
    fn test_save_and_load_channel_groups() {
        // Arrange
        let (conn, _dir) = setup_db();
        let groups = vec![
            CachedChannelGroup {
                ch_gid: 1,
                ch_group_name: String::from("テレビ 関東"),
                ch_group_order: 1200,
            },
            CachedChannelGroup {
                ch_gid: 2,
                ch_group_name: String::from("BSデジタル"),
                ch_group_order: 3000,
            },
        ];

        // Act
        save_channel_groups(&conn, &groups).unwrap();
        let loaded = load_channel_groups(&conn).unwrap();

        // Assert (ordered by ch_group_order)
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].ch_gid, 1);
        assert_eq!(loaded[0].ch_group_name, "テレビ 関東");
        assert_eq!(loaded[1].ch_gid, 2);
    }

    #[test]
    fn test_save_and_load_channels() {
        // Arrange
        let (conn, _dir) = setup_db();
        let groups = vec![CachedChannelGroup {
            ch_gid: 1,
            ch_group_name: String::from("テレビ 関東"),
            ch_group_order: 1200,
        }];
        save_channel_groups(&conn, &groups).unwrap();

        let channels = vec![
            CachedChannel {
                ch_id: 3,
                ch_gid: Some(1),
                ch_name: String::from("フジテレビ"),
            },
            CachedChannel {
                ch_id: 1,
                ch_gid: Some(1),
                ch_name: String::from("NHK総合"),
            },
        ];

        // Act
        save_channels(&conn, &channels).unwrap();
        let loaded = load_channels(&conn).unwrap();

        // Assert (ordered by ch_id)
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].ch_id, 1);
        assert_eq!(loaded[0].ch_name, "NHK総合");
        assert_eq!(loaded[1].ch_id, 3);
    }

    #[test]
    fn test_save_replaces_existing() {
        // Arrange
        let (conn, _dir) = setup_db();
        let groups = vec![CachedChannelGroup {
            ch_gid: 1,
            ch_group_name: String::from("Old"),
            ch_group_order: 100,
        }];
        save_channel_groups(&conn, &groups).unwrap();

        // Act
        let new_groups = vec![CachedChannelGroup {
            ch_gid: 2,
            ch_group_name: String::from("New"),
            ch_group_order: 200,
        }];
        save_channel_groups(&conn, &new_groups).unwrap();
        let loaded = load_channel_groups(&conn).unwrap();

        // Assert
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].ch_gid, 2);
        assert_eq!(loaded[0].ch_group_name, "New");
    }

    #[test]
    fn test_load_empty_tables() {
        // Arrange
        let (conn, _dir) = setup_db();

        // Act
        let groups = load_channel_groups(&conn).unwrap();
        let channels = load_channels(&conn).unwrap();

        // Assert
        assert!(groups.is_empty());
        assert!(channels.is_empty());
    }
}
