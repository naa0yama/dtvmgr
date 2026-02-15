//! `AppConfig` struct and TOML read/write.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Top-level application configuration.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AppConfig {
    /// Channel selection settings.
    #[serde(default)]
    pub channels: ChannelsConfig,
}

/// Channel selection configuration.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ChannelsConfig {
    /// Selected channel IDs (Syoboi `ChID`).
    #[serde(default)]
    pub selected: Vec<u32>,
}

impl AppConfig {
    /// Loads config from a TOML file. Returns default if file does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))
    }

    /// Saves config to a TOML file, creating parent directories if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation or file write fails.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        let content = toml::to_string_pretty(self).context("failed to serialize config to TOML")?;
        std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn test_default_config() {
        // Arrange & Act
        let config = AppConfig::default();

        // Assert
        assert!(config.channels.selected.is_empty());
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        // Arrange
        let config = AppConfig {
            channels: ChannelsConfig {
                selected: vec![1, 2, 3, 7, 19],
            },
        };

        // Act
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();

        // Assert
        assert_eq!(parsed, config);
    }

    #[test]
    fn test_load_nonexistent_returns_default() {
        // Arrange
        let path = Path::new("/tmp/dtvmgr_test_nonexistent_config.toml");

        // Act
        let config = AppConfig::load(path).unwrap();

        // Assert
        assert_eq!(config, AppConfig::default());
    }

    #[test]
    fn test_save_and_load() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let config = AppConfig {
            channels: ChannelsConfig {
                selected: vec![1, 3, 7],
            },
        };

        // Act
        config.save(&path).unwrap();
        let loaded = AppConfig::load(&path).unwrap();

        // Assert
        assert_eq!(loaded, config);
    }

    #[test]
    fn test_load_partial_config() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();

        // Act
        let config = AppConfig::load(&path).unwrap();

        // Assert
        assert_eq!(config, AppConfig::default());
    }
}
