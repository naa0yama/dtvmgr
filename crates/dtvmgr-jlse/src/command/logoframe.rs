//! Wrapper for the `logoframe` external command.
//!
//! Detects logo frames and selects the appropriate logo file based on
//! channel information.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use tracing::debug;

use crate::types::Channel;

/// Run `logoframe` to detect logo frames in the AVS file.
///
/// Selects the logo file from `logo_dir` based on `channel`, then runs:
/// `logoframe <avs> -logo <logo> -oa <txt> -o <avs_out>`
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits with a
/// non-zero status code.
pub fn run(
    binary: &Path,
    avs_file: &Path,
    txt_output: &Path,
    avs_output: &Path,
    logo_dir: &Path,
    channel: Option<&Channel>,
) -> Result<()> {
    let logo_path = select_logo(logo_dir, channel)?;
    let args = build_args(avs_file, txt_output, avs_output, &logo_path);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    super::run(binary, &os_args)
}

/// Run `logoframe` with stderr captured via `on_log` callback.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits with a
/// non-zero status code.
pub fn run_logged(
    binary: &Path,
    avs_file: &Path,
    txt_output: &Path,
    avs_output: &Path,
    logo_dir: &Path,
    channel: Option<&Channel>,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    let logo_path = select_logo(logo_dir, channel)?;
    let args = build_args(avs_file, txt_output, avs_output, &logo_path);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    super::run_logged(binary, &os_args, on_log)
}

/// Select the most appropriate logo file from `logo_dir` for the given channel.
///
/// Priority order:
/// 1. `channel.install` (if non-empty)
/// 2. `channel.short`   (if non-empty)
/// 3. `channel.recognize` (if non-empty)
/// 4. `SID<service_id>`
///
/// # Errors
///
/// Returns an error if no channel is provided or no matching logo file
/// is found. Without a logo, `chapter_exe` accuracy degrades
/// significantly.
pub fn select_logo(logo_dir: &Path, channel: Option<&Channel>) -> Result<PathBuf> {
    let Some(ch) = channel else {
        bail!("no channel detected; cannot select logo file");
    };

    let candidates = [
        &ch.install,
        &ch.short,
        &ch.recognize,
        &format!("SID{}", ch.service_id),
    ];

    for name in candidates {
        if name.is_empty() {
            continue;
        }
        if let Some(path) = find_logo(logo_dir, name) {
            debug!(logo = %path.display(), name, "found logo file");
            return Ok(path);
        }
    }

    bail!(
        "no matching logo file found in {} for channel (short={}, service_id={})",
        logo_dir.display(),
        ch.short,
        ch.service_id,
    );
}

/// Search `logo_dir` for a logo file matching `name`.
///
/// Search order:
/// 1. `<name>.lgd`
/// 2. `<name>.lgd2`
/// 3. If `name` starts with `"SID"`, scan for `<name>-*.lgd` and return
///    the one with the highest numeric suffix.
#[must_use]
pub fn find_logo(logo_dir: &Path, name: &str) -> Option<PathBuf> {
    // Try exact .lgd
    let lgd = logo_dir.join(format!("{name}.lgd"));
    if lgd.is_file() {
        return Some(lgd);
    }

    // Try exact .lgd2
    let lgd2 = logo_dir.join(format!("{name}.lgd2"));
    if lgd2.is_file() {
        return Some(lgd2);
    }

    // SID prefix: scan for <name>-<N>.lgd with highest N
    if name.starts_with("SID") {
        return find_sid_logo(logo_dir, name);
    }

    None
}

/// Scan `logo_dir` for `<prefix>-<N>.lgd` files and return the one with
/// the highest numeric suffix.
fn find_sid_logo(logo_dir: &Path, prefix: &str) -> Option<PathBuf> {
    let pattern = format!("{prefix}-");
    let suffix = ".lgd";

    let Ok(entries) = std::fs::read_dir(logo_dir) else {
        return None;
    };

    let mut best: Option<(u32, PathBuf)> = None;

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        let Some(rest) = name_str.strip_prefix(&*pattern) else {
            continue;
        };
        let Some(inner) = rest.strip_suffix(suffix) else {
            continue;
        };

        if let Ok(n) = inner.parse::<u32>()
            && best.as_ref().is_none_or(|(best_n, _)| n > *best_n)
        {
            best = Some((n, entry.path()));
        }
    }

    best.map(|(_, path)| path)
}

/// Build the argument list for `logoframe`.
///
/// Format: `<avs> -logo <logo> -oa <txt_output> -o <avs_output>`
#[must_use]
pub fn build_args(
    avs_file: &Path,
    txt_output: &Path,
    avs_output: &Path,
    logo_path: &Path,
) -> Vec<String> {
    vec![
        avs_file.display().to_string(),
        "-logo".to_owned(),
        logo_path.display().to_string(),
        "-oa".to_owned(),
        txt_output.display().to_string(),
        "-o".to_owned(),
        avs_output.display().to_string(),
    ]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use std::path::Path;

    use super::*;

    #[test]
    fn test_build_args() {
        // Arrange
        let avs = Path::new("/out/in_org.avs");
        let txt = Path::new("/out/obs_logoframe.txt");
        let avs_out = Path::new("/out/obs_logo_erase.avs");
        let logo = Path::new("/logo/BS1.lgd");

        // Act
        let args = build_args(avs, txt, avs_out, logo);

        // Assert
        assert_eq!(
            args,
            vec![
                "/out/in_org.avs",
                "-logo",
                "/logo/BS1.lgd",
                "-oa",
                "/out/obs_logoframe.txt",
                "-o",
                "/out/obs_logo_erase.avs",
            ]
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_find_logo_lgd() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("BS1.lgd"), "").unwrap();

        // Act
        let result = find_logo(tmp.path(), "BS1");

        // Assert
        assert_eq!(result, Some(tmp.path().join("BS1.lgd")));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_find_logo_lgd2() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("NHK.lgd2"), "").unwrap();

        // Act
        let result = find_logo(tmp.path(), "NHK");

        // Assert
        assert_eq!(result, Some(tmp.path().join("NHK.lgd2")));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_find_logo_lgd_preferred_over_lgd2() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("BS1.lgd"), "").unwrap();
        std::fs::write(tmp.path().join("BS1.lgd2"), "").unwrap();

        // Act
        let result = find_logo(tmp.path(), "BS1");

        // Assert — .lgd takes priority
        assert_eq!(result, Some(tmp.path().join("BS1.lgd")));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_find_logo_sid_highest_number() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("SID101-1.lgd"), "").unwrap();
        std::fs::write(tmp.path().join("SID101-3.lgd"), "").unwrap();
        std::fs::write(tmp.path().join("SID101-2.lgd"), "").unwrap();

        // Act
        let result = find_logo(tmp.path(), "SID101");

        // Assert
        assert_eq!(result, Some(tmp.path().join("SID101-3.lgd")));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_find_logo_not_found() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();

        // Act
        let result = find_logo(tmp.path(), "MISSING");

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_select_logo_no_channel() {
        // Arrange
        let logo_dir = Path::new("/logo");

        // Act
        let result = select_logo(logo_dir, None);

        // Assert
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("no channel detected"),
            "expected 'no channel detected' in: {err}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_select_logo_install_priority() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("inst.lgd"), "").unwrap();
        std::fs::write(tmp.path().join("BS1.lgd"), "").unwrap();
        let ch = Channel {
            install: "inst".to_owned(),
            short: "BS1".to_owned(),
            recognize: "NHK".to_owned(),
            service_id: "101".to_owned(),
        };

        // Act
        let result = select_logo(tmp.path(), Some(&ch)).unwrap();

        // Assert — install has highest priority
        assert_eq!(result, tmp.path().join("inst.lgd"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_select_logo_short_when_install_empty() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("BS1.lgd"), "").unwrap();
        let ch = Channel {
            install: String::new(),
            short: "BS1".to_owned(),
            recognize: "NHK".to_owned(),
            service_id: "101".to_owned(),
        };

        // Act
        let result = select_logo(tmp.path(), Some(&ch)).unwrap();

        // Assert
        assert_eq!(result, tmp.path().join("BS1.lgd"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_select_logo_recognize_fallback() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("NHK.lgd"), "").unwrap();
        let ch = Channel {
            install: String::new(),
            short: String::new(),
            recognize: "NHK".to_owned(),
            service_id: "101".to_owned(),
        };

        // Act
        let result = select_logo(tmp.path(), Some(&ch)).unwrap();

        // Assert
        assert_eq!(result, tmp.path().join("NHK.lgd"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_select_logo_sid_fallback() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("SID101-1.lgd"), "").unwrap();
        let ch = Channel {
            install: String::new(),
            short: String::new(),
            recognize: String::new(),
            service_id: "101".to_owned(),
        };

        // Act
        let result = select_logo(tmp.path(), Some(&ch)).unwrap();

        // Assert
        assert_eq!(result, tmp.path().join("SID101-1.lgd"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_find_logo_sid_no_match() {
        // Arrange — SID prefix but no matching files
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("SID999-1.lgd"), "").unwrap();

        // Act
        let result = find_logo(tmp.path(), "SID101");

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_sid_logo_nonexistent_dir() {
        // Arrange — directory does not exist
        let result = find_sid_logo(Path::new("/nonexistent/logo/dir"), "SID101");

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_find_logo_non_sid_no_match() {
        // Arrange — name does not start with SID, no .lgd/.lgd2 files
        let tmp = tempfile::tempdir().unwrap();

        // Act
        let result = find_logo(tmp.path(), "MISSING");

        // Assert — returns None without SID scan
        assert_eq!(result, None);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_select_logo_nothing_found() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let ch = Channel {
            install: String::new(),
            short: "BS1".to_owned(),
            recognize: "NHK".to_owned(),
            service_id: "999".to_owned(),
        };

        // Act
        let result = select_logo(tmp.path(), Some(&ch));

        // Assert — returns error when no logo found
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("no matching logo file found"),
            "expected 'no matching logo file found' in: {err}"
        );
    }
}
