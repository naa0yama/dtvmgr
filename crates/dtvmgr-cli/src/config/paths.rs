//! Config directory resolution.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Config file name.
const CONFIG_FILE_NAME: &str = "dtvmgr.toml";

/// Resolves the data directory for database and other files.
///
/// Priority:
/// 1. `--config` specified → parent directory of the config file
/// 2. CWD `./dtvmgr.toml` exists with marker keys → CWD
/// 3. `None` (falls back to `dtvmgr-db` default `~/.local/share/dtvmgr/`)
///
/// # Errors
///
/// Returns an error if CWD detection fails.
pub fn resolve_data_dir(config: Option<&PathBuf>) -> Result<Option<PathBuf>> {
    if let Some(c) = config {
        let abs = std::fs::canonicalize(c)
            .with_context(|| format!("failed to canonicalize config path: {}", c.display()))?;
        return Ok(abs.parent().map(PathBuf::from));
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
/// 1. `--config` specified → that path directly (canonicalized)
/// 2. CWD `./dtvmgr.toml` exists with `syoboi` or `tmdb` top-level key → CWD path
/// 3. `~/.config/dtvmgr/dtvmgr.toml`
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined (when `config` is `None`)
/// or CWD detection fails.
pub fn resolve_config_path(config: Option<&PathBuf>) -> Result<PathBuf> {
    if let Some(c) = config {
        // Return the path as-is if it doesn't exist yet (e.g. for init),
        // otherwise canonicalize to resolve relative paths.
        return if c.exists() {
            std::fs::canonicalize(c)
                .with_context(|| format!("failed to canonicalize config path: {}", c.display()))
        } else {
            Ok(c.clone())
        };
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
    #[cfg_attr(miri, ignore)]
    fn test_resolve_with_config_file() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let config_file = dir.path().join("dtvmgr.toml");
        std::fs::write(&config_file, "").unwrap();

        // Act
        let path = resolve_config_path(Some(&config_file)).unwrap();

        // Assert: returns the canonicalized file path directly
        assert_eq!(path, std::fs::canonicalize(&config_file).unwrap());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_resolve_default() {
        // Arrange & Act
        let path = resolve_config_path(None).unwrap();

        // Assert
        assert!(path.ends_with(".config/dtvmgr/dtvmgr.toml"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_resolve_data_dir_with_config_file() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let config_file = dir.path().join("dtvmgr.toml");
        std::fs::write(&config_file, "").unwrap();

        // Act
        let result = resolve_data_dir(Some(&config_file)).unwrap();

        // Assert: returns the parent directory of the config file
        assert_eq!(result, Some(std::fs::canonicalize(dir.path()).unwrap()));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_resolve_data_dir_none_without_cwd_config() {
        // Arrange & Act
        let result = resolve_data_dir(None).unwrap();

        // Assert: no CWD config in test env → None
        assert!(result.is_none());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_resolve_config_path_default_contains_config_file() {
        // Arrange & Act
        let path = resolve_config_path(None).unwrap();

        // Assert: path ends with the config file name
        assert!(path.ends_with(CONFIG_FILE_NAME));
        assert!(path.is_absolute());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_resolve_with_relative_path() {
        // Arrange: create config file in a temp dir, then use a relative path
        let dir = tempfile::tempdir().unwrap();
        let config_file = dir.path().join("dtvmgr.toml");
        std::fs::write(&config_file, "").unwrap();

        // Use "./dtvmgr.toml" as a relative path by changing to the temp dir
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let relative = PathBuf::from("./dtvmgr.toml");

        // Act
        let path = resolve_config_path(Some(&relative)).unwrap();

        // Cleanup: restore original directory
        std::env::set_current_dir(&original_dir).unwrap();

        // Assert: relative path is resolved to absolute
        assert!(path.is_absolute());
        assert_eq!(path, std::fs::canonicalize(&config_file).unwrap());
    }

    #[test]
    fn test_resolve_config_path_nonexistent_returns_as_is() {
        // Arrange
        let config_file = PathBuf::from("/nonexistent/path/dtvmgr.toml");

        // Act
        let path = resolve_config_path(Some(&config_file)).unwrap();

        // Assert: non-existent path is returned as-is
        assert_eq!(path, config_file);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_detect_cwd_config_with_normalize_key() {
        // Arrange: create a dtvmgr.toml with `normalize` marker key
        let dir = tempfile::tempdir().unwrap();
        let config_file = dir.path().join("dtvmgr.toml");
        std::fs::write(&config_file, "[normalize]\nenabled = true\n").unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        // Act
        let result = detect_cwd_config();

        // Cleanup
        std::env::set_current_dir(&original_dir).unwrap();

        // Assert: should detect the config because it has the `normalize` key
        let path = result.unwrap().unwrap();
        assert!(path.ends_with("dtvmgr.toml"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_detect_cwd_config_no_marker_keys() {
        // Arrange: create a dtvmgr.toml WITHOUT any marker keys
        let dir = tempfile::tempdir().unwrap();
        let config_file = dir.path().join("dtvmgr.toml");
        std::fs::write(&config_file, "[other]\nkey = \"value\"\n").unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        // Act
        let result = detect_cwd_config();

        // Cleanup
        std::env::set_current_dir(&original_dir).unwrap();

        // Assert: should return None because no marker keys found
        assert!(result.unwrap().is_none());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_detect_cwd_config_with_syoboi_key() {
        // Arrange: create a dtvmgr.toml with `syoboi` marker key
        let dir = tempfile::tempdir().unwrap();
        let config_file = dir.path().join("dtvmgr.toml");
        std::fs::write(&config_file, "[syoboi]\ntid = 1234\n").unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        // Act
        let result = detect_cwd_config();

        // Cleanup
        std::env::set_current_dir(&original_dir).unwrap();

        // Assert
        let path = result.unwrap().unwrap();
        assert!(path.ends_with("dtvmgr.toml"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_resolve_config_path_cwd_with_tmdb_key() {
        // Arrange: create dtvmgr.toml with `tmdb` marker key in CWD
        let dir = tempfile::tempdir().unwrap();
        let config_file = dir.path().join("dtvmgr.toml");
        std::fs::write(&config_file, "[tmdb]\napi_key = \"test\"\n").unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        // Act
        let result = resolve_config_path(None);

        // Cleanup
        std::env::set_current_dir(&original_dir).unwrap();

        // Assert: should find CWD config
        let path = result.unwrap();
        assert!(path.ends_with("dtvmgr.toml"));
        assert!(path.is_absolute());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_resolve_data_dir_cwd_detection() {
        // Arrange: create dtvmgr.toml with marker key in CWD
        let dir = tempfile::tempdir().unwrap();
        let config_file = dir.path().join("dtvmgr.toml");
        std::fs::write(&config_file, "[syoboi]\ntid = 1\n").unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        // Act
        let result = resolve_data_dir(None);

        // Cleanup
        std::env::set_current_dir(&original_dir).unwrap();

        // Assert: should return the CWD as data dir
        let data_dir = result.unwrap().unwrap();
        assert!(data_dir.is_absolute());
    }
}
