//! Storage statistics collection for monitoring disk usage.
//!
//! Collects filesystem capacity and per-directory file sizes so operators
//! can track TS accumulation and remaining headroom in recording / encode
//! directories.

use std::path::Path;

use tracing::{info, warn};

// ── Types ────────────────────────────────────────────────────

/// Filesystem and directory-level statistics.
///
/// `total_bytes` and `free_bytes` come from the underlying filesystem
/// (`statvfs`), while `used_bytes`, `usage_ratio`, and `file_count` are
/// calculated per-directory by summing individual file sizes.
#[derive(Debug, Clone)]
#[allow(clippy::module_name_repetitions)]
pub struct StorageStats {
    /// Directory path that was inspected.
    pub path: String,
    /// Available bytes for unprivileged users (filesystem-level, `f_bavail`).
    pub free_bytes: u64,
    /// Total filesystem capacity in bytes.
    pub total_bytes: u64,
    /// Sum of file sizes in this directory (directory-level).
    pub used_bytes: u64,
    /// Directory usage ratio against filesystem capacity (`used_bytes / total_bytes`).
    pub usage_ratio: f64,
    /// Number of regular files in the top-level directory.
    pub file_count: u64,
}

// ── Public API ───────────────────────────────────────────────

/// Collect storage statistics for `dir`.
///
/// `total_bytes` / `free_bytes` are filesystem-level (from `statvfs`).
/// `used_bytes` / `file_count` are directory-level (file size sum).
///
/// Returns `None` when the directory does not exist or `statvfs` fails,
/// logging a warning in either case.
#[allow(clippy::cast_precision_loss, clippy::as_conversions)]
pub fn collect_storage_stats(dir: &Path) -> Option<StorageStats> {
    if !dir.is_dir() {
        warn!(dir = %dir.display(), "storage stats: directory does not exist");
        return None;
    }

    let Some(vfs) = statvfs(dir) else {
        warn!(dir = %dir.display(), "storage stats: statvfs failed");
        return None;
    };

    let block_size = vfs.frsize;
    let total_bytes = vfs.blocks.saturating_mul(block_size);
    let free_bytes = vfs.bavail.saturating_mul(block_size);

    let (file_count, used_bytes) = scan_dir(dir);

    let usage_ratio = if total_bytes == 0 {
        0.0
    } else {
        used_bytes as f64 / total_bytes as f64
    };

    Some(StorageStats {
        path: dir.to_string_lossy().into_owned(),
        free_bytes,
        total_bytes,
        used_bytes,
        usage_ratio,
        file_count,
    })
}

/// Returns filesystem free bytes for `dir`, or `None` if inaccessible.
#[must_use]
pub fn free_bytes(dir: &Path) -> Option<u64> {
    let vfs = statvfs(dir)?;
    Some(vfs.bavail.saturating_mul(vfs.frsize))
}

/// Log storage statistics for the TS input directory and (optionally) the
/// encoded output directory.
pub fn log_storage_stats(ts_dir: &Path, out_dir: Option<&Path>) {
    if let Some(stats) = collect_storage_stats(ts_dir) {
        log_stats(&stats, "ts_input");
    }
    if let Some(dir) = out_dir
        && let Some(stats) = collect_storage_stats(dir)
    {
        log_stats(&stats, "encoded_output");
    }
}

// ── Internals ────────────────────────────────────────────────

fn log_stats(stats: &StorageStats, role: &str) {
    let usage_pct = stats.usage_ratio * 100.0;
    info!(
        dir = %stats.path,
        role,
        free_bytes = stats.free_bytes,
        total_bytes = stats.total_bytes,
        used_bytes = stats.used_bytes,
        usage_pct = format_args!("{usage_pct:.1}"),
        file_count = stats.file_count,
        "storage stats",
    );
}

/// Minimal subset of `libc::statvfs` we care about.
#[derive(Debug)]
#[allow(clippy::struct_field_names)]
struct StatVfs {
    /// Fragment size in bytes.
    frsize: u64,
    /// Total data blocks in filesystem.
    blocks: u64,
    /// Free blocks available to unprivileged users.
    bavail: u64,
}

/// Safe wrapper around `libc::statvfs`.
fn statvfs(path: &Path) -> Option<StatVfs> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut buf = MaybeUninit::<libc::statvfs>::uninit();

    // SAFETY: `buf` is a valid pointer to uninitialised `statvfs`;
    // `c_path` is a valid NUL-terminated C string.
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), buf.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }

    // SAFETY: `statvfs` returned 0, so `buf` is fully initialised.
    let vfs = unsafe { buf.assume_init() };

    Some(StatVfs {
        frsize: vfs.f_frsize,
        blocks: vfs.f_blocks,
        bavail: vfs.f_bavail,
    })
}

/// Scan top-level regular files in `dir`, returning `(count, total_size)`.
fn scan_dir(dir: &Path) -> (u64, u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut count: u64 = 0;
    let mut size: u64 = 0;
    for entry in entries.filter_map(Result::ok) {
        if entry.file_type().is_ok_and(|ft| ft.is_file()) {
            count = count.saturating_add(1);
            if let Ok(meta) = entry.metadata() {
                size = size.saturating_add(meta.len());
            }
        }
    }
    (count, size)
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_collect_storage_stats_existing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.ts"), b"aaaa").unwrap();
        std::fs::write(tmp.path().join("b.ts"), b"bb").unwrap();
        std::fs::create_dir_all(tmp.path().join("subdir")).unwrap();

        let stats = collect_storage_stats(tmp.path()).unwrap();

        assert_eq!(stats.path, tmp.path().to_string_lossy());
        assert!(stats.total_bytes > 0);
        assert!(stats.free_bytes <= stats.total_bytes);
        // used_bytes = sum of file sizes in directory (4 + 2 = 6).
        assert_eq!(stats.used_bytes, 6);
        assert_eq!(stats.file_count, 2);
        // usage_ratio = 6 / total_bytes — tiny but positive.
        assert!(stats.usage_ratio > 0.0);
        assert!(stats.usage_ratio < 1.0);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_collect_storage_stats_nonexistent_dir() {
        let result = collect_storage_stats(&PathBuf::from("/nonexistent_dir_12345"));
        assert!(result.is_none());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_statvfs_wrapper() {
        let result = statvfs(Path::new("/tmp"));
        assert!(result.is_some());
        let vfs = result.unwrap();
        assert!(vfs.frsize > 0);
        assert!(vfs.blocks > 0);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_scan_dir_counts_only_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("x.ts"), b"hello").unwrap();
        std::fs::create_dir_all(tmp.path().join("sub")).unwrap();

        let (count, size) = scan_dir(tmp.path());

        assert_eq!(count, 1);
        assert_eq!(size, 5);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_log_storage_stats_valid_dirs() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.ts"), b"data").unwrap();

        // Act & Assert — should not panic
        log_storage_stats(tmp.path(), None);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_log_storage_stats_with_out_dir() {
        // Arrange
        let ts_dir = tempfile::tempdir().unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        std::fs::write(ts_dir.path().join("a.ts"), b"ts").unwrap();
        std::fs::write(out_dir.path().join("b.mp4"), b"enc").unwrap();

        // Act & Assert — should not panic
        log_storage_stats(ts_dir.path(), Some(out_dir.path()));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_log_storage_stats_nonexistent_dirs() {
        // Act & Assert — should not panic (warns internally)
        log_storage_stats(
            &PathBuf::from("/nonexistent_ts_dir_12345"),
            Some(Path::new("/nonexistent_out_dir_12345")),
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_free_bytes_tmp() {
        // Act
        let result = free_bytes(Path::new("/tmp"));

        // Assert
        assert!(result.is_some());
        assert!(result.unwrap() > 0);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_free_bytes_nonexistent() {
        // Act
        let result = free_bytes(Path::new("/nonexistent_path_12345"));

        // Assert
        assert!(result.is_none());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_collect_storage_stats_empty_dir() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();

        // Act
        let stats = collect_storage_stats(tmp.path()).unwrap();

        // Assert
        assert_eq!(stats.used_bytes, 0);
        assert_eq!(stats.file_count, 0);
        assert!((stats.usage_ratio - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_scan_dir_nonexistent() {
        // Act
        let (count, size) = scan_dir(Path::new("/nonexistent_scan_dir_12345"));

        // Assert
        assert_eq!(count, 0);
        assert_eq!(size, 0);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_statvfs_nonexistent() {
        // Act
        let result = statvfs(Path::new("/nonexistent_statvfs_12345"));

        // Assert
        assert!(result.is_none());
    }
}
