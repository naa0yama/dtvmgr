//! Common helpers for spawning external commands.
//!
//! All wrappers in this module use synchronous [`std::process::Command`].
//! The pipeline steps are sequential, so async provides no benefit here.

pub mod chapter_exe;
pub mod ffmpeg;
pub mod ffprobe;
pub mod join_logo_scp;
pub mod logoframe;
pub mod tsdivider;

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
