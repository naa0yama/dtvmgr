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

/// Upserts channel groups into the cache. Returns the number of rows changed.
///
/// Uses `INSERT ... ON CONFLICT(ch_gid) DO UPDATE SET` to update existing rows.
/// Only updates when a value has actually changed.
///
/// # Errors
///
/// Returns an error if the database operation fails.
pub fn upsert_channel_groups(conn: &Connection, groups: &[CachedChannelGroup]) -> Result<usize> {
    let tx = conn
        .unchecked_transaction()
        .context("failed to begin transaction")?;

    let mut stmt = tx
        .prepare(
            "INSERT INTO channel_groups (ch_gid, ch_group_name, ch_group_order)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(ch_gid) DO UPDATE SET
                ch_group_name  = excluded.ch_group_name,
                ch_group_order = excluded.ch_group_order
            WHERE channel_groups.ch_group_name  != excluded.ch_group_name
               OR channel_groups.ch_group_order != excluded.ch_group_order",
        )
        .context("failed to prepare channel_groups upsert")?;

    let mut changed: usize = 0;
    for g in groups {
        let rows = stmt
            .execute(rusqlite::params![
                g.ch_gid,
                g.ch_group_name,
                g.ch_group_order
            ])
            .with_context(|| format!("failed to upsert channel_group {}", g.ch_gid))?;
        changed = changed.saturating_add(rows);
    }

    drop(stmt);
    tx.commit().context("failed to commit channel_groups")?;
    Ok(changed)
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

/// Upserts channels into the cache. Returns the number of rows changed.
///
/// Uses `INSERT ... ON CONFLICT(ch_id) DO UPDATE SET` to update existing rows.
/// When `ch_gid` is `None`, existing `ch_gid` is preserved via `COALESCE`.
/// Only updates when a value has actually changed.
///
/// # Errors
///
/// Returns an error if the database operation fails.
#[allow(clippy::module_name_repetitions)]
pub fn upsert_channels(conn: &Connection, channels: &[CachedChannel]) -> Result<usize> {
    let tx = conn
        .unchecked_transaction()
        .context("failed to begin transaction")?;

    let mut stmt = tx
        .prepare(
            "INSERT INTO channels (ch_id, ch_gid, ch_name)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(ch_id) DO UPDATE SET
                ch_gid  = COALESCE(excluded.ch_gid, channels.ch_gid),
                ch_name = excluded.ch_name
            WHERE channels.ch_name != excluded.ch_name
               OR COALESCE(channels.ch_gid, -1) != COALESCE(excluded.ch_gid, channels.ch_gid, -1)",
        )
        .context("failed to prepare channels upsert")?;

    let mut changed: usize = 0;
    for ch in channels {
        let rows = stmt
            .execute(rusqlite::params![ch.ch_id, ch.ch_gid, ch.ch_name])
            .with_context(|| format!("failed to upsert channel {}", ch.ch_id))?;
        changed = changed.saturating_add(rows);
    }

    drop(stmt);
    tx.commit().context("failed to commit channels")?;
    Ok(changed)
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
    use crate::programs::{CachedProgram, upsert_programs};

    fn setup_db() -> (Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(Some(&dir.path().to_path_buf())).unwrap();
        (conn, dir)
    }

    #[test]
    fn test_upsert_and_load_channel_groups() {
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
        let changed = upsert_channel_groups(&conn, &groups).unwrap();
        let loaded = load_channel_groups(&conn).unwrap();

        // Assert (ordered by ch_group_order)
        assert_eq!(changed, 2);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].ch_gid, 1);
        assert_eq!(loaded[0].ch_group_name, "テレビ 関東");
        assert_eq!(loaded[1].ch_gid, 2);
    }

    #[test]
    fn test_upsert_and_load_channels() {
        // Arrange
        let (conn, _dir) = setup_db();
        let groups = vec![CachedChannelGroup {
            ch_gid: 1,
            ch_group_name: String::from("テレビ 関東"),
            ch_group_order: 1200,
        }];
        upsert_channel_groups(&conn, &groups).unwrap();

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
        let changed = upsert_channels(&conn, &channels).unwrap();
        let loaded = load_channels(&conn).unwrap();

        // Assert (ordered by ch_id)
        assert_eq!(changed, 2);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].ch_id, 1);
        assert_eq!(loaded[0].ch_name, "NHK総合");
        assert_eq!(loaded[1].ch_id, 3);
    }

    #[test]
    fn test_upsert_channel_groups_updates_existing() {
        // Arrange
        let (conn, _dir) = setup_db();
        let groups = vec![CachedChannelGroup {
            ch_gid: 1,
            ch_group_name: String::from("Old"),
            ch_group_order: 100,
        }];
        upsert_channel_groups(&conn, &groups).unwrap();

        // Act — update existing + add new
        let new_groups = vec![
            CachedChannelGroup {
                ch_gid: 1,
                ch_group_name: String::from("Updated"),
                ch_group_order: 200,
            },
            CachedChannelGroup {
                ch_gid: 2,
                ch_group_name: String::from("New"),
                ch_group_order: 300,
            },
        ];
        let changed = upsert_channel_groups(&conn, &new_groups).unwrap();
        let loaded = load_channel_groups(&conn).unwrap();

        // Assert — both rows present, old row updated
        assert_eq!(changed, 2);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].ch_gid, 1);
        assert_eq!(loaded[0].ch_group_name, "Updated");
        assert_eq!(loaded[0].ch_group_order, 200);
        assert_eq!(loaded[1].ch_gid, 2);
    }

    #[test]
    fn test_upsert_channel_groups_unchanged_returns_zero() {
        // Arrange
        let (conn, _dir) = setup_db();
        let groups = vec![CachedChannelGroup {
            ch_gid: 1,
            ch_group_name: String::from("Same"),
            ch_group_order: 100,
        }];
        upsert_channel_groups(&conn, &groups).unwrap();

        // Act — upsert identical data
        let changed = upsert_channel_groups(&conn, &groups).unwrap();

        // Assert
        assert_eq!(changed, 0);
    }

    #[test]
    fn test_upsert_channels_updates_existing() {
        // Arrange
        let (conn, _dir) = setup_db();
        let groups = vec![CachedChannelGroup {
            ch_gid: 1,
            ch_group_name: String::from("Group"),
            ch_group_order: 100,
        }];
        upsert_channel_groups(&conn, &groups).unwrap();

        let channels = vec![CachedChannel {
            ch_id: 10,
            ch_gid: Some(1),
            ch_name: String::from("Old Name"),
        }];
        upsert_channels(&conn, &channels).unwrap();

        // Act — update name
        let updated = vec![CachedChannel {
            ch_id: 10,
            ch_gid: Some(1),
            ch_name: String::from("New Name"),
        }];
        let changed = upsert_channels(&conn, &updated).unwrap();
        let loaded = load_channels(&conn).unwrap();

        // Assert
        assert_eq!(changed, 1);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].ch_name, "New Name");
    }

    #[test]
    fn test_upsert_channels_with_programs_present() {
        // Arrange — insert channel group, channel, title, and program
        let (conn, _dir) = setup_db();
        let groups = vec![CachedChannelGroup {
            ch_gid: 1,
            ch_group_name: String::from("Group"),
            ch_group_order: 100,
        }];
        upsert_channel_groups(&conn, &groups).unwrap();

        let channels = vec![CachedChannel {
            ch_id: 10,
            ch_gid: Some(1),
            ch_name: String::from("TestCh"),
        }];
        upsert_channels(&conn, &channels).unwrap();

        // Insert a title (FK for programs.tid)
        let titles = vec![crate::titles::CachedTitle {
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
            last_update: String::from("2025-01-01 00:00:00"),
            tmdb_original_name: None,
            tmdb_name: None,
            tmdb_alt_titles: None,
        }];
        crate::titles::upsert_titles(&conn, &titles).unwrap();

        let programs = vec![CachedProgram {
            pid: 1000,
            tid: 100,
            ch_id: 10,
            tmdb_episode_id: None,
            st_time: String::from("2025-01-01 00:00:00"),
            st_offset: None,
            ed_time: String::from("2025-01-01 00:30:00"),
            count: Some(1),
            sub_title: None,
            flag: None,
            deleted: None,
            warn: None,
            revision: None,
            last_update: Some(String::from("2025-01-01 00:00:00")),
            st_sub_title: None,
            duration_min: None,
        }];
        upsert_programs(&conn, &programs).unwrap();

        // Act — upsert channels again (should NOT fail with FK error)
        let updated = vec![CachedChannel {
            ch_id: 10,
            ch_gid: Some(1),
            ch_name: String::from("Updated Name"),
        }];
        let result = upsert_channels(&conn, &updated);

        // Assert
        assert!(result.is_ok());
        let loaded = load_channels(&conn).unwrap();
        assert_eq!(loaded[0].ch_name, "Updated Name");
    }

    #[test]
    fn test_upsert_channels_preserves_ch_gid_when_null() {
        // Arrange — insert channel with ch_gid
        let (conn, _dir) = setup_db();
        let groups = vec![CachedChannelGroup {
            ch_gid: 1,
            ch_group_name: String::from("Group"),
            ch_group_order: 100,
        }];
        upsert_channel_groups(&conn, &groups).unwrap();

        let channels = vec![CachedChannel {
            ch_id: 10,
            ch_gid: Some(1),
            ch_name: String::from("TestCh"),
        }];
        upsert_channels(&conn, &channels).unwrap();

        // Act — upsert with ch_gid: None
        let updated = vec![CachedChannel {
            ch_id: 10,
            ch_gid: None,
            ch_name: String::from("TestCh"),
        }];
        upsert_channels(&conn, &updated).unwrap();
        let loaded = load_channels(&conn).unwrap();

        // Assert — ch_gid preserved
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].ch_gid, Some(1));
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
