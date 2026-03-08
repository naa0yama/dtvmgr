//! Common helpers for spawning `TSDuck` external commands.
//!
//! All wrappers in this module use synchronous [`std::process::Command`].

use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;
use tracing::debug;

/// Spawn a command, inherit stdout/stderr, and check exit status.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits with a
/// non-zero status code.
fn run(program: &Path, args: &[&OsStr]) -> Result<()> {
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
fn run_capture(program: &Path, args: &[&OsStr]) -> Result<String> {
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

    let stdout = String::from_utf8(output.stdout)
        .with_context(|| format!("{} produced non-UTF-8 stdout", program.display()))?;

    if stdout.trim().is_empty() {
        bail!(
            "{} produced no output (input may not be a valid TS file)",
            program.display(),
        );
    }

    Ok(stdout)
}

/// Extract EIT XML from a TS file using `tstables`.
///
/// Command: `tstables --japan --pid 0x12 --xml-output - <input>`
///
/// # Errors
///
/// Returns an error if `tstables` cannot be spawned or exits with a
/// non-zero status code.
pub fn extract_eit(binary: &Path, input_file: &Path) -> Result<String> {
    let args = build_eit_args(input_file);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    run_capture(binary, &os_args)
}

/// Build the argument list for `tstables` EIT extraction.
#[must_use]
pub fn build_eit_args(input_file: &Path) -> Vec<String> {
    vec![
        "--japan".to_owned(),
        "--pid".to_owned(),
        "0x12".to_owned(),
        "--xml-output".to_owned(),
        "-".to_owned(),
        input_file.display().to_string(),
    ]
}

/// Extract PAT XML from a TS file using `tstables`.
///
/// Command: `tstables --japan --pid 0 --xml-output - <input>`
///
/// PAT is on PID 0 and is typically very small, so this is fast even for
/// large recordings.
///
/// # Errors
///
/// Returns an error if `tstables` cannot be spawned or exits with a
/// non-zero status code.
pub fn extract_pat(binary: &Path, input_file: &Path) -> Result<String> {
    let args = build_pat_args(input_file);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    run_capture(binary, &os_args)
}

/// Build the argument list for `tstables` PAT extraction.
#[must_use]
pub fn build_pat_args(input_file: &Path) -> Vec<String> {
    vec![
        "--japan".to_owned(),
        "--pid".to_owned(),
        "0".to_owned(),
        "--xml-output".to_owned(),
        "-".to_owned(),
        input_file.display().to_string(),
    ]
}

/// Extract EIT p/f XML from a TS file using `tstables`.
///
/// Command: `tstables --japan --pid 0x12 --tid 0x4E --max-tables 4 --xml-output - <input>`
///
/// Unlike [`extract_eit`], this filters to EIT p/f actual only (`--tid 0x4E`)
/// and limits the number of tables for early termination.
///
/// # Errors
///
/// Returns an error if `tstables` cannot be spawned or exits with a
/// non-zero status code.
pub fn extract_eit_pf(binary: &Path, input_file: &Path) -> Result<String> {
    let args = build_eit_pf_args(input_file);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    run_capture(binary, &os_args)
}

/// Build the argument list for `tstables` EIT p/f extraction.
#[must_use]
pub fn build_eit_pf_args(input_file: &Path) -> Vec<String> {
    vec![
        "--japan".to_owned(),
        "--pid".to_owned(),
        "0x12".to_owned(),
        "--tid".to_owned(),
        "0x4E".to_owned(),
        "--max-tables".to_owned(),
        "4".to_owned(),
        "--xml-output".to_owned(),
        "-".to_owned(),
        input_file.display().to_string(),
    ]
}

/// Extract EIT p/f XML from an in-memory TS chunk using `tstables`.
///
/// Writes `chunk` to a temporary file (since `tstables` requires a file path),
/// runs [`extract_eit_pf`], and cleans up the temp file automatically.
///
/// # Errors
///
/// Returns an error if the temp file cannot be created/written, or if
/// `tstables` fails.
pub fn extract_eit_from_chunk(binary: &Path, chunk: &[u8]) -> Result<String> {
    let mut tmp = NamedTempFile::new().context("failed to create temp file for chunk")?;
    tmp.write_all(chunk)
        .context("failed to write chunk to temp file")?;
    tmp.flush().context("failed to flush chunk temp file")?;

    extract_eit_pf(binary, tmp.path())
}

/// Filter a TS file by service ID using `tsp`.
///
/// Command: `tsp --japan -I file <input> -P zap <sid> -O file <output>`
///
/// # Errors
///
/// Returns an error if `tsp` cannot be spawned or exits with a
/// non-zero status code.
pub fn filter_service(
    binary: &Path,
    input_file: &Path,
    output_file: &Path,
    sid: &str,
) -> Result<()> {
    let args = build_filter_service_args(input_file, output_file, sid);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    run(binary, &os_args)
}

/// Build the argument list for `tsp` service filtering.
#[must_use]
pub fn build_filter_service_args(input_file: &Path, output_file: &Path, sid: &str) -> Vec<String> {
    vec![
        "--japan".to_owned(),
        "-I".to_owned(),
        "file".to_owned(),
        input_file.display().to_string(),
        "-P".to_owned(),
        "zap".to_owned(),
        sid.to_owned(),
        "-O".to_owned(),
        "file".to_owned(),
        output_file.display().to_string(),
    ]
}

#[cfg(test)]
pub(crate) mod test_utils {
    /// Creates a temporary executable shell script with the given body.
    ///
    /// Serialises file creation to avoid `ETXTBSY` on overlay
    /// filesystems where concurrent `close` + `execve` can race.
    #[cfg(unix)]
    pub fn write_script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        use std::sync::Mutex;

        static LOCK: Mutex<()> = Mutex::new(());
        let _guard = LOCK.lock().unwrap();

        let script = dir.join(name);
        let mut f = std::fs::File::create(&script).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f.set_permissions(std::fs::Permissions::from_mode(0o755))
            .unwrap();
        f.sync_all().unwrap();
        drop(f);
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
    fn test_build_eit_args() {
        // Arrange
        let input = Path::new("/rec/video.ts");

        // Act
        let args = build_eit_args(input);

        // Assert
        assert_eq!(
            args,
            vec![
                "--japan",
                "--pid",
                "0x12",
                "--xml-output",
                "-",
                "/rec/video.ts"
            ]
        );
    }

    #[test]
    fn test_build_filter_service_args() {
        // Arrange
        let input = Path::new("/rec/video.ts");
        let output = Path::new("/out/filtered.ts");
        let sid = "1024";

        // Act
        let args = build_filter_service_args(input, output, sid);

        // Assert
        assert_eq!(
            args,
            vec![
                "--japan",
                "-I",
                "file",
                "/rec/video.ts",
                "-P",
                "zap",
                "1024",
                "-O",
                "file",
                "/out/filtered.ts",
            ]
        );
    }

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
    fn test_run_failure() {
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
    fn test_run_capture_empty_output() {
        // Arrange — script succeeds but produces no output
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "empty.sh", "#!/bin/sh\nexit 0\n");

        // Act
        let result = run_capture(&script, &[]);

        // Assert
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("produced no output"),
            "expected 'produced no output' in: {err}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_eit_success() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(
            dir.path(),
            "tstables",
            "#!/bin/sh\necho '<tsduck></tsduck>'\n",
        );

        // Act
        let result = extract_eit(&script, Path::new("/rec/video.ts"));

        // Assert
        assert_eq!(result.unwrap(), "<tsduck></tsduck>\n");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_eit_failure() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "tstables", "#!/bin/sh\nexit 1\n");

        // Act
        let result = extract_eit(&script, Path::new("/rec/video.ts"));

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn test_build_eit_pf_args() {
        // Arrange
        let input = Path::new("/rec/chunk.ts");

        // Act
        let args = build_eit_pf_args(input);

        // Assert
        assert_eq!(
            args,
            vec![
                "--japan",
                "--pid",
                "0x12",
                "--tid",
                "0x4E",
                "--max-tables",
                "4",
                "--xml-output",
                "-",
                "/rec/chunk.ts",
            ]
        );
    }

    #[test]
    fn test_build_pat_args() {
        // Arrange
        let input = Path::new("/rec/video.ts");

        // Act
        let args = build_pat_args(input);

        // Assert
        assert_eq!(
            args,
            vec![
                "--japan",
                "--pid",
                "0",
                "--xml-output",
                "-",
                "/rec/video.ts"
            ]
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_pat_success() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(
            dir.path(),
            "tstables",
            "#!/bin/sh\necho '<tsduck><PAT/></tsduck>'\n",
        );

        // Act
        let result = extract_pat(&script, Path::new("/rec/video.ts"));

        // Assert
        assert_eq!(result.unwrap(), "<tsduck><PAT/></tsduck>\n");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_pat_failure() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "tstables", "#!/bin/sh\nexit 1\n");

        // Act
        let result = extract_pat(&script, Path::new("/rec/video.ts"));

        // Assert
        assert!(result.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_eit_pf_success() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(
            dir.path(),
            "tstables",
            "#!/bin/sh\necho '<tsduck></tsduck>'\n",
        );

        // Act
        let result = extract_eit_pf(&script, Path::new("/rec/video.ts"));

        // Assert
        assert_eq!(result.unwrap(), "<tsduck></tsduck>\n");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_eit_pf_failure() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "tstables", "#!/bin/sh\nexit 1\n");

        // Act
        let result = extract_eit_pf(&script, Path::new("/rec/video.ts"));

        // Assert
        assert!(result.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_eit_from_chunk_success() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(
            dir.path(),
            "tstables",
            "#!/bin/sh\necho '<tsduck></tsduck>'\n",
        );
        let chunk = vec![0x47; 188 * 10];

        // Act
        let result = extract_eit_from_chunk(&script, &chunk);

        // Assert
        assert_eq!(result.unwrap(), "<tsduck></tsduck>\n");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_extract_eit_from_chunk_failure() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "tstables", "#!/bin/sh\nexit 1\n");
        let chunk = vec![0x47; 188 * 10];

        // Act
        let result = extract_eit_from_chunk(&script, &chunk);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_filter_service_success() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "tsp", "#!/bin/sh\nexit 0\n");

        // Act
        let result = filter_service(
            &script,
            Path::new("/rec/video.ts"),
            Path::new("/out/filtered.ts"),
            "1024",
        );

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_filter_service_failure() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let script = write_script(dir.path(), "tsp", "#!/bin/sh\nexit 1\n");

        // Act
        let result = filter_service(
            &script,
            Path::new("/rec/video.ts"),
            Path::new("/out/filtered.ts"),
            "1024",
        );

        // Assert
        assert!(result.is_err());
    }
}
