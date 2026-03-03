//! Manual TMDB mapping file support.
//!
//! Loads `dtvmgr.mapping.toml` containing user-defined tid-to-TMDB mappings,
//! with fallback to a remote GitHub-hosted version.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// GitHub raw URL for the shared mapping file.
const MAPPING_GITHUB_URL: &str =
    "https://raw.githubusercontent.com/naa0yama/dtvmgr/main/dtvmgr.mapping.toml";

/// Filename for the local mapping file.
const MAPPING_FILENAME: &str = "dtvmgr.mapping.toml";

/// A single manual tid-to-TMDB mapping entry.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[allow(clippy::module_name_repetitions)]
pub struct MappingEntry {
    /// Syoboi title ID.
    pub tid: u32,
    /// Title name (for readability).
    pub name: String,
    /// TMDB series ID. Use 0 as placeholder for unfilled entries.
    pub tmdb_series_id: u64,
    /// Optional TMDB season number.
    #[serde(default)]
    pub tmdb_season_number: Option<u32>,
    /// TMDB season ID. Use 0 as placeholder for unfilled entries.
    #[serde(default)]
    pub tmdb_season_id: u64,
}

/// Top-level mapping file structure.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[allow(clippy::module_name_repetitions)]
pub struct MappingFile {
    /// List of manual mapping entries.
    #[serde(default)]
    pub mappings: Vec<MappingEntry>,
}

impl MappingFile {
    /// Load a mapping file from the given path.
    ///
    /// Returns an empty `MappingFile` if the file does not exist.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                mappings: Vec::new(),
            });
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))
    }

    /// Save mapping file to the given path as pretty-printed TOML.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        let content =
            toml::to_string_pretty(self).context("failed to serialize mapping file to TOML")?;
        std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
    }

    /// Merge new entries that are not already present (by tid).
    ///
    /// Adds each `(tid, name)` as a placeholder entry with `tmdb_series_id = 0`.
    /// Sorts all entries by tid ascending after merging.
    pub fn merge_new_entries(&mut self, new_entries: &[(u32, &str)]) {
        let existing_tids: HashSet<u32> = self.mappings.iter().map(|e| e.tid).collect();
        for (tid, name) in new_entries {
            if !existing_tids.contains(tid) {
                self.mappings.push(MappingEntry {
                    tid: *tid,
                    name: name.to_string(),
                    tmdb_series_id: 0,
                    tmdb_season_number: None,
                    tmdb_season_id: 0,
                });
            }
        }
        self.mappings.sort_by_key(|e| e.tid);
    }

    /// Remove entries whose tid is in the given exclusion set.
    pub fn remove_excluded(&mut self, excluded_tids: &HashSet<u32>) {
        self.mappings.retain(|e| !excluded_tids.contains(&e.tid));
    }

    /// Build a lookup index from tid to mapping entry.
    pub fn build_index(&self) -> HashMap<u32, &MappingEntry> {
        self.mappings.iter().map(|e| (e.tid, e)).collect()
    }
}

/// Load mapping from local file, falling back to GitHub download.
///
/// Looks for `dtvmgr.mapping.toml` in `config_dir`. If not found,
/// attempts to download from the GitHub repository and saves the
/// result to the local file.
///
/// Returns the loaded mapping and the local file path.
pub async fn load_or_fetch(config_dir: &Path) -> Result<(MappingFile, PathBuf)> {
    let local_path = config_dir.join(MAPPING_FILENAME);
    if local_path.exists() {
        tracing::info!(local_path = %local_path.display(), "Loading local mapping file");
        let mapping = MappingFile::load(&local_path)
            .with_context(|| format!("failed to load mapping from {}", local_path.display()))?;
        return Ok((mapping, local_path));
    }

    tracing::info!("Local mapping file not found, fetching from GitHub");
    let mapping = match fetch_from_url(MAPPING_GITHUB_URL).await {
        Ok(mapping) => {
            tracing::info!(
                entries = mapping.mappings.len(),
                "Fetched mapping from GitHub"
            );
            mapping
        }
        Err(e) => {
            tracing::warn!("Failed to fetch mapping from GitHub: {e:#}");
            MappingFile {
                mappings: Vec::new(),
            }
        }
    };

    mapping
        .save(&local_path)
        .with_context(|| format!("failed to save mapping to {}", local_path.display()))?;

    Ok((mapping, local_path))
}

/// Fetch and parse a mapping file from a URL.
async fn fetch_from_url(url: &str) -> Result<MappingFile> {
    let body = reqwest::get(url)
        .await
        .with_context(|| format!("failed to fetch mapping from {url}"))?
        .error_for_status()
        .with_context(|| format!("HTTP error fetching mapping from {url}"))?
        .text()
        .await
        .with_context(|| format!("failed to read response body from {url}"))?;
    toml::from_str(&body).with_context(|| format!("failed to parse mapping TOML from {url}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_nonexistent_returns_empty() {
        // Arrange
        let path = Path::new("/tmp/nonexistent_mapping_test_file.toml");

        // Act
        let result = MappingFile::load(path).unwrap();

        // Assert
        assert!(result.mappings.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_valid_toml() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        std::fs::write(
            &path,
            r#"
[[mappings]]
tid = 12345
name = "Test Title"
tmdb_series_id = 67890
tmdb_season_number = 1

[[mappings]]
tid = 99999
name = "Another Title"
tmdb_series_id = 11111
tmdb_season_number = 2
"#,
        )
        .unwrap();

        // Act
        let result = MappingFile::load(&path).unwrap();

        // Assert
        assert_eq!(result.mappings.len(), 2);
        assert_eq!(result.mappings[0].tid, 12345);
        assert_eq!(result.mappings[0].name, "Test Title");
        assert_eq!(result.mappings[0].tmdb_series_id, 67890);
        assert_eq!(result.mappings[0].tmdb_season_number, Some(1));
        assert_eq!(result.mappings[1].tid, 99999);
        assert_eq!(result.mappings[1].tmdb_series_id, 11111);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_without_season_number() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        std::fs::write(
            &path,
            r#"
[[mappings]]
tid = 12345
name = "No Season"
tmdb_series_id = 67890
"#,
        )
        .unwrap();

        // Act
        let result = MappingFile::load(&path).unwrap();

        // Assert
        assert_eq!(result.mappings.len(), 1);
        assert_eq!(result.mappings[0].tmdb_season_number, None);
    }

    #[test]
    fn test_build_index() {
        // Arrange
        let mapping = MappingFile {
            mappings: vec![
                MappingEntry {
                    tid: 100,
                    name: "Title A".to_owned(),
                    tmdb_series_id: 200,
                    tmdb_season_number: Some(1),
                    tmdb_season_id: 0,
                },
                MappingEntry {
                    tid: 300,
                    name: "Title B".to_owned(),
                    tmdb_series_id: 400,
                    tmdb_season_number: None,
                    tmdb_season_id: 0,
                },
            ],
        };

        // Act
        let index = mapping.build_index();

        // Assert
        assert_eq!(index.len(), 2);
        assert_eq!(index[&100].tmdb_series_id, 200);
        assert_eq!(index[&300].tmdb_series_id, 400);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_save_and_load_roundtrip() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roundtrip.toml");
        let original = MappingFile {
            mappings: vec![
                MappingEntry {
                    tid: 100,
                    name: "Title A".to_owned(),
                    tmdb_series_id: 200,
                    tmdb_season_number: Some(1),
                    tmdb_season_id: 0,
                },
                MappingEntry {
                    tid: 300,
                    name: "Title B".to_owned(),
                    tmdb_series_id: 0,
                    tmdb_season_number: None,
                    tmdb_season_id: 0,
                },
            ],
        };

        // Act
        original.save(&path).unwrap();
        let loaded = MappingFile::load(&path).unwrap();

        // Assert
        assert_eq!(original, loaded);
    }

    #[test]
    fn test_merge_new_entries_basic() {
        // Arrange
        let mut mapping = MappingFile {
            mappings: vec![MappingEntry {
                tid: 100,
                name: "Existing".to_owned(),
                tmdb_series_id: 999,
                tmdb_season_number: None,
                tmdb_season_id: 0,
            }],
        };

        // Act
        mapping.merge_new_entries(&[(200, "New Title"), (50, "Earlier Title")]);

        // Assert
        assert_eq!(mapping.mappings.len(), 3);
        assert_eq!(mapping.mappings[0].tid, 50);
        assert_eq!(mapping.mappings[0].name, "Earlier Title");
        assert_eq!(mapping.mappings[0].tmdb_series_id, 0);
        assert_eq!(mapping.mappings[1].tid, 100);
        assert_eq!(mapping.mappings[2].tid, 200);
    }

    #[test]
    fn test_merge_new_entries_skips_existing() {
        // Arrange
        let mut mapping = MappingFile {
            mappings: vec![MappingEntry {
                tid: 100,
                name: "Existing".to_owned(),
                tmdb_series_id: 999,
                tmdb_season_number: Some(2),
                tmdb_season_id: 0,
            }],
        };

        // Act
        mapping.merge_new_entries(&[(100, "Duplicate"), (200, "New")]);

        // Assert
        assert_eq!(mapping.mappings.len(), 2);
        assert_eq!(mapping.mappings[0].tid, 100);
        assert_eq!(mapping.mappings[0].tmdb_series_id, 999);
        assert_eq!(mapping.mappings[1].tid, 200);
    }

    #[test]
    fn test_merge_new_entries_empty() {
        // Arrange
        let mut mapping = MappingFile {
            mappings: vec![MappingEntry {
                tid: 100,
                name: "Existing".to_owned(),
                tmdb_series_id: 999,
                tmdb_season_number: None,
                tmdb_season_id: 0,
            }],
        };

        // Act
        mapping.merge_new_entries(&[]);

        // Assert
        assert_eq!(mapping.mappings.len(), 1);
        assert_eq!(mapping.mappings[0].tid, 100);
    }

    #[test]
    fn test_merge_new_entries_sorted_by_tid() {
        // Arrange
        let mut mapping = MappingFile {
            mappings: Vec::new(),
        };

        // Act
        mapping.merge_new_entries(&[(500, "E"), (100, "A"), (300, "C")]);

        // Assert
        assert_eq!(mapping.mappings.len(), 3);
        assert_eq!(mapping.mappings[0].tid, 100);
        assert_eq!(mapping.mappings[1].tid, 300);
        assert_eq!(mapping.mappings[2].tid, 500);
    }

    #[test]
    fn test_remove_excluded_basic() {
        // Arrange
        let mut mapping = MappingFile {
            mappings: vec![
                MappingEntry {
                    tid: 100,
                    name: "Keep".to_owned(),
                    tmdb_series_id: 1,
                    tmdb_season_number: None,
                    tmdb_season_id: 0,
                },
                MappingEntry {
                    tid: 200,
                    name: "Remove".to_owned(),
                    tmdb_series_id: 2,
                    tmdb_season_number: None,
                    tmdb_season_id: 0,
                },
                MappingEntry {
                    tid: 300,
                    name: "Keep2".to_owned(),
                    tmdb_series_id: 3,
                    tmdb_season_number: None,
                    tmdb_season_id: 0,
                },
            ],
        };

        // Act
        mapping.remove_excluded(&HashSet::from([200]));

        // Assert
        assert_eq!(mapping.mappings.len(), 2);
        assert_eq!(mapping.mappings[0].tid, 100);
        assert_eq!(mapping.mappings[1].tid, 300);
    }

    #[test]
    fn test_remove_excluded_empty_set() {
        // Arrange
        let mut mapping = MappingFile {
            mappings: vec![MappingEntry {
                tid: 100,
                name: "Keep".to_owned(),
                tmdb_series_id: 1,
                tmdb_season_number: None,
                tmdb_season_id: 0,
            }],
        };

        // Act
        mapping.remove_excluded(&HashSet::new());

        // Assert: nothing removed
        assert_eq!(mapping.mappings.len(), 1);
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn test_load_or_fetch_local_exists() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(MAPPING_FILENAME);
        std::fs::write(
            &path,
            r#"
[[mappings]]
tid = 42
name = "Local"
tmdb_series_id = 999
"#,
        )
        .unwrap();

        // Act
        let (mapping, returned_path) = load_or_fetch(dir.path()).await.unwrap();

        // Assert
        assert_eq!(mapping.mappings.len(), 1);
        assert_eq!(mapping.mappings[0].tid, 42);
        assert_eq!(returned_path, path);
    }
}
