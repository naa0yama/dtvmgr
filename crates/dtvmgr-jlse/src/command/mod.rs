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
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use dtvmgr_tsduck::command::apply_pdeathsig;
use tracing::debug;

/// Spawn a command, inherit stdout/stderr, and check exit status.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits with a
/// non-zero status code.
pub fn run(program: &Path, args: &[&OsStr]) -> Result<()> {
    debug!(cmd = %program.display(), ?args, "running command");

    let mut cmd = Command::new(program);
    cmd.args(args);
    apply_pdeathsig(&mut cmd);
    let status = cmd
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

/// Spawn a command, capture stderr line-by-line via callback, and check
/// exit status.
///
/// Stdout is suppressed. Each stderr line is forwarded to `on_log`.
/// Used by TUI mode to display command output without corrupting the
/// alternate screen.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits with a
/// non-zero status code.
pub fn run_logged(program: &Path, args: &[&OsStr], on_log: &dyn Fn(&str)) -> Result<()> {
    debug!(cmd = %program.display(), ?args, "running command (logged)");

    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::null()).stderr(Stdio::piped());
    apply_pdeathsig(&mut cmd);
    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn {}", program.display()))?;

    if let Some(stderr) = child.stderr.take() {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let line =
                line.with_context(|| format!("failed to read stderr from {}", program.display()))?;
            on_log(&line);
        }
    }

    let status = child
        .wait()
        .with_context(|| format!("failed to wait for {}", program.display()))?;

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

    let mut cmd = Command::new(program);
    cmd.args(args);
    apply_pdeathsig(&mut cmd);
    let output = cmd
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
    ///
    /// Uses a subprocess (`sh -c "cat > file && chmod …"`) to write the
    /// script so that the writing fd is owned by a child process.  When
    /// `wait()` returns, the child has fully exited and the kernel has
    /// reaped all its fds, guaranteeing `i_writecount == 0` on the inode.
    /// This avoids `ETXTBSY` on overlayfs (Docker containers in CI).
    #[cfg(unix)]
    pub fn write_script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::io::Write;

        let target = dir.join(name);

        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(
                "cat > '{}' && chmod 755 '{}'",
                target.display(),
                target.display()
            ))
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        // Close stdin after writing to signal EOF to cat.
        {
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(body.as_bytes()).unwrap();
        }

        let status = child.wait().unwrap();
        assert!(status.success());

        target
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

    // ── run_logged ───────────────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_logged_success_captures_stderr() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(
            dir.path(),
            "logged.sh",
            "#!/bin/sh\necho line1 >&2\necho line2 >&2\nexit 0\n",
        );
        let lines = std::cell::RefCell::new(Vec::new());

        // Act
        let result = run_logged(&script, &[], &|line| {
            lines.borrow_mut().push(line.to_owned());
        });

        // Assert
        assert!(result.is_ok());
        assert_eq!(*lines.borrow(), vec!["line1", "line2"]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_logged_failure() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(
            dir.path(),
            "fail_logged.sh",
            "#!/bin/sh\necho err >&2\nexit 7\n",
        );
        let lines = std::cell::RefCell::new(Vec::new());

        // Act
        let result = run_logged(&script, &[], &|line| {
            lines.borrow_mut().push(line.to_owned());
        });

        // Assert
        let err = result.unwrap_err().to_string();
        assert!(err.contains('7'), "expected exit code 7 in: {err}");
        assert_eq!(*lines.borrow(), vec!["err"]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_run_logged_nonexistent() {
        // Act
        let result = run_logged(Path::new("/nonexistent/binary"), &[], &|_| {});

        // Assert
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("failed to spawn"),
            "expected 'failed to spawn' in: {err}"
        );
    }
}
