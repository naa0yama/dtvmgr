//! Common helpers for spawning external commands.
//!
//! All wrappers in this module use synchronous [`std::process::Command`].
//! The pipeline steps are sequential, so async provides no benefit here.

pub mod chapter_exe;
pub mod ffmpeg;
pub mod ffprobe;
pub mod join_logo_scp;
pub mod logoframe;

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use tracing::debug;

/// Spawn a command, inherit stdout/stderr, and check exit status.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits with a
/// non-zero status code.
pub fn run(program: &Path, args: &[&OsStr]) -> Result<()> {
    debug!(cmd = %program.display(), ?args, "running command");

    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to spawn {}", program.display()))?;

    if !status.success() {
        bail!(
            "{} exited with {}",
            program.display(),
            status
                .code()
                .map_or_else(|| "signal".to_owned(), |c| c.to_string()),
        );
    }

    Ok(())
}

/// Spawn a command, capture stdout as a [`String`], and inherit stderr.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned, exits with a
/// non-zero status code, or stdout is not valid UTF-8.
pub fn run_capture(program: &Path, args: &[&OsStr]) -> Result<String> {
    debug!(cmd = %program.display(), ?args, "running command (capture)");

    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn {}", program.display()))?;

    if !output.status.success() {
        bail!(
            "{} exited with {}",
            program.display(),
            output
                .status
                .code()
                .map_or_else(|| "signal".to_owned(), |c| c.to_string()),
        );
    }

    String::from_utf8(output.stdout)
        .with_context(|| format!("{} produced non-UTF-8 stdout", program.display()))
}

#[cfg(test)]
pub(crate) mod test_utils {
    /// Creates a temporary executable shell script with the given body.
    #[cfg(unix)]
    pub fn write_script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join(name);
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::Path;

    use super::test_utils::write_script;
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_success() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "ok.sh", "#!/bin/sh\nexit 0\n");

        // Act
        let result = run(&script, &[]);

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_failure_exit_code() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "fail.sh", "#!/bin/sh\nexit 42\n");

        // Act
        let result = run(&script, &[]);

        // Assert
        let err = result.unwrap_err().to_string();
        assert!(err.contains("42"), "expected exit code 42 in: {err}");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_nonexistent_binary() {
        // Act
        let result = run(Path::new("/nonexistent/binary"), &[]);

        // Assert
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("failed to spawn"),
            "expected 'failed to spawn' in: {err}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_capture_success() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "echo.sh", "#!/bin/sh\necho hello\n");

        // Act
        let result = run_capture(&script, &[]);

        // Assert
        assert_eq!(result.unwrap(), "hello\n");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_capture_failure() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "fail.sh", "#!/bin/sh\nexit 1\n");

        // Act
        let result = run_capture(&script, &[]);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_capture_nonexistent() {
        // Act
        let result = run_capture(Path::new("/nonexistent/binary"), &[]);

        // Assert
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("failed to spawn"),
            "expected 'failed to spawn' in: {err}"
        );
    }
}
