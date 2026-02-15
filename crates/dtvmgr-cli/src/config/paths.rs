//! Config directory resolution.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Resolves the config file path.
///
/// - If `dir` is `Some`, returns `{dir}/config.toml`.
/// - Otherwise returns `~/.config/dtvmgr/config.toml`.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined (when `dir` is `None`).
pub fn resolve_config_path(dir: Option<&PathBuf>) -> Result<PathBuf> {
    if let Some(d) = dir {
        return Ok(d.join("config.toml"));
    }

    let home = std::env::var("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("dtvmgr")
        .join("config.toml"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn test_resolve_with_dir() {
        // Arrange
        let dir = PathBuf::from("/tmp/myproject");

        // Act
        let path = resolve_config_path(Some(&dir)).unwrap();

        // Assert
        assert_eq!(path, PathBuf::from("/tmp/myproject/config.toml"));
    }

    #[test]
    fn test_resolve_default() {
        // Arrange & Act
        let path = resolve_config_path(None).unwrap();

        // Assert
        assert!(path.ends_with(".config/dtvmgr/config.toml"));
    }
}
