//! Wrapper for the `tsdivider` external command.
//!
//! Splits a TS file to remove multi-channel audio streams.

use std::ffi::OsStr;
use std::path::Path;

use anyhow::Result;

/// Run `tsdivider` to split `input_file` into `output_file`.
///
/// Command: `tsdivider -i <input> --overlap_front 0 -o <output>`
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits with a
/// non-zero status code.
pub fn run(binary: &Path, input_file: &Path, output_file: &Path) -> Result<()> {
    let args = build_args(input_file, output_file);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    super::run(binary, &os_args)
}

/// Build the argument list for `tsdivider`.
#[must_use]
pub fn build_args(input_file: &Path, output_file: &Path) -> Vec<String> {
    vec![
        "-i".to_owned(),
        input_file.display().to_string(),
        "--overlap_front".to_owned(),
        "0".to_owned(),
        "-o".to_owned(),
        output_file.display().to_string(),
    ]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::Path;

    use super::*;

    #[test]
    fn test_build_args() {
        // Arrange
        let input = Path::new("/rec/video.ts");
        let output = Path::new("/out/video_split.ts");

        // Act
        let args = build_args(input, output);

        // Assert
        assert_eq!(
            args,
            vec![
                "-i",
                "/rec/video.ts",
                "--overlap_front",
                "0",
                "-o",
                "/out/video_split.ts",
            ]
        );
    }
}
