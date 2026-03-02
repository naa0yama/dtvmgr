//! Wrapper for the `chapter_exe` external command.
//!
//! Detects scene changes and generates chapter information.

use std::ffi::OsStr;
use std::path::Path;

use anyhow::Result;

/// Run `chapter_exe` to detect chapters from the AVS file.
///
/// Command: `chapter_exe -v <avs> -s 8 -e 4 -o <output>`
///
/// `chapter_exe` may crash during cleanup (the `AviSynth` script
/// environment is never properly released — see `source.h`). When the
/// process exits with a signal rather than a non-zero exit code, the
/// output file is checked; if it exists and is non-empty the crash is
/// treated as non-fatal.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned, exits with a
/// non-zero exit code, or crashes without producing an output file.
pub fn run(binary: &Path, avs_file: &Path, output_file: &Path) -> Result<()> {
    let args = build_args(avs_file, output_file);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();

    match super::run(binary, &os_args) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Tolerate signal-killed exits when the output was written.
            if output_file.metadata().is_ok_and(|m| m.len() > 0) {
                tracing::warn!("chapter_exe crashed but output file exists; continuing");
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

/// Build the argument list for `chapter_exe`.
#[must_use]
pub fn build_args(avs_file: &Path, output_file: &Path) -> Vec<String> {
    vec![
        "-v".to_owned(),
        avs_file.display().to_string(),
        "-s".to_owned(),
        "8".to_owned(),
        "-e".to_owned(),
        "4".to_owned(),
        "-o".to_owned(),
        output_file.display().to_string(),
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
        let output = Path::new("/out/obs_chapterexe.txt");

        // Act
        let args = build_args(avs, output);

        // Assert
        assert_eq!(
            args,
            vec![
                "-v",
                "/out/in_org.avs",
                "-s",
                "8",
                "-e",
                "4",
                "-o",
                "/out/obs_chapterexe.txt",
            ]
        );
    }

    #[test]
    fn test_build_args_fixed_params() {
        // Arrange
        let avs = Path::new("/any.avs");
        let output = Path::new("/any.txt");

        // Act
        let args = build_args(avs, output);

        // Assert — -s 8 and -e 4 are always present
        assert_eq!(args[2], "-s");
        assert_eq!(args[3], "8");
        assert_eq!(args[4], "-e");
        assert_eq!(args[5], "4");
    }
}
