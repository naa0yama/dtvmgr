//! Config directory resolution.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Config file name.
const CONFIG_FILE_NAME: &str = "dtvmgr.toml";

/// Resolves the data directory for database and other files.
///
/// Priority:
/// 1. `--dir` specified → `{dir}`
/// 2. CWD `./dtvmgr.toml` exists with marker keys → CWD
/// 3. `None` (falls back to `dtvmgr-db` default `~/.local/share/dtvmgr/`)
///
/// # Errors
///
/// Returns an error if CWD detection fails.
pub fn resolve_data_dir(dir: Option<&PathBuf>) -> Result<Option<PathBuf>> {
    if let Some(d) = dir {
        return Ok(Some(d.clone()));
    }

    if let Some(cwd_config) = detect_cwd_config()? {
        // Parent of `{cwd}/dtvmgr.toml` is the CWD itself.
        return Ok(cwd_config.parent().map(PathBuf::from));
    }

    Ok(None)
}

/// Resolves the config file path.
///
/// Priority:
/// 1. `--dir` specified → `{dir}/dtvmgr.toml`
/// 2. CWD `./dtvmgr.toml` exists with `syoboi` or `tmdb` top-level key → CWD path
/// 3. `~/.config/dtvmgr/dtvmgr.toml`
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined (when `dir` is `None`)
/// or CWD detection fails.
pub fn resolve_config_path(dir: Option<&PathBuf>) -> Result<PathBuf> {
    if let Some(d) = dir {
        return Ok(d.join(CONFIG_FILE_NAME));
    }

    // Try CWD auto-detection
    if let Some(cwd_path) = detect_cwd_config()? {
        return Ok(cwd_path);
    }

    let home = std::env::var("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("dtvmgr")
        .join(CONFIG_FILE_NAME))
}

/// Checks if `./dtvmgr.toml` exists in CWD and contains marker keys.
fn detect_cwd_config() -> Result<Option<PathBuf>> {
    let path = PathBuf::from(CONFIG_FILE_NAME);
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let value: toml::Value =
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;

    if let toml::Value::Table(table) = &value
        && (table.contains_key("syoboi")
            || table.contains_key("tmdb")
            || table.contains_key("normalize"))
    {
        return Ok(Some(
            std::env::current_dir()
                .context("failed to get current directory")?
                .join(CONFIG_FILE_NAME),
        ));
    }

    Ok(None)
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
        assert_eq!(path, PathBuf::from("/tmp/myproject/dtvmgr.toml"));
    }

    #[test]
    fn test_resolve_default() {
        // Arrange & Act
        let path = resolve_config_path(None).unwrap();

        // Assert
        assert!(path.ends_with(".config/dtvmgr/dtvmgr.toml"));
    }

    #[test]
    fn test_resolve_data_dir_with_dir() {
        // Arrange
        let dir = PathBuf::from("/tmp/myproject");

        // Act
        let result = resolve_data_dir(Some(&dir)).unwrap();

        // Assert
        assert_eq!(result, Some(PathBuf::from("/tmp/myproject")));
    }

    #[test]
    fn test_resolve_data_dir_none_without_cwd_config() {
        // Arrange & Act
        let result = resolve_data_dir(None).unwrap();

        // Assert: no CWD config in test env → None
        assert!(result.is_none());
    }
}
