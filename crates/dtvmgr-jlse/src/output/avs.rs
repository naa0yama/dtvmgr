//! AVS file concatenation.
//!
//! Combines multiple AVS scripts into a single file by appending their
//! contents with `Import()` directives.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};

/// Concatenate multiple AVS files into a single output file using
/// `Import()` directives.
///
/// Each input file is referenced as `Import("<path>")` in the output,
/// preserving the order of `input_files`.
///
/// # Errors
///
/// Returns an error if the output file cannot be written.
pub fn concat(output_path: &Path, input_files: &[&Path]) -> Result<()> {
    let mut content = String::new();

    for input in input_files {
        writeln!(content, "Import(\"{}\")", input.display())
            .with_context(|| "failed to format AVS import")?;
    }

    std::fs::write(output_path, &content)
        .with_context(|| format!("failed to write AVS file: {}", output_path.display()))?;

    Ok(())
}

/// Create a cut-CM AVS file that imports the input AVS and cut AVS.
///
/// Generates: `Import("<input_avs>")` + `Import("<cut_avs>")`
///
/// # Errors
///
/// Returns an error if the output file cannot be written.
pub fn create_cutcm(output: &Path, input_avs: &Path, cut_avs: &Path) -> Result<()> {
    concat(output, &[input_avs, cut_avs])
}

/// Create a cut-CM AVS with logo erasure that imports input, logo erase,
/// and cut AVS files.
///
/// Generates: `Import("<input_avs>")` + `Import("<logo_erase_avs>")` +
/// `Import("<cut_avs>")`
///
/// # Errors
///
/// Returns an error if the output file cannot be written.
pub fn create_cutcm_logo(
    output: &Path,
    input_avs: &Path,
    logo_erase_avs: &Path,
    cut_avs: &Path,
) -> Result<()> {
    concat(output, &[input_avs, logo_erase_avs, cut_avs])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use std::path::Path;

    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_concat_avs_two_files() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("combined.avs");
        let a = Path::new("/out/in_org.avs");
        let b = Path::new("/out/obs_cut.avs");

        // Act
        concat(&output, &[a, b]).unwrap();

        // Assert
        let content = std::fs::read_to_string(&output).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "Import(\"/out/in_org.avs\")");
        assert_eq!(lines[1], "Import(\"/out/obs_cut.avs\")");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_concat_avs_three_files() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("combined.avs");
        let a = Path::new("/out/in_org.avs");
        let b = Path::new("/out/obs_logo_erase.avs");
        let c = Path::new("/out/obs_cut.avs");

        // Act
        concat(&output, &[a, b, c]).unwrap();

        // Assert
        let content = std::fs::read_to_string(&output).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "Import(\"/out/in_org.avs\")");
        assert_eq!(lines[1], "Import(\"/out/obs_logo_erase.avs\")");
        assert_eq!(lines[2], "Import(\"/out/obs_cut.avs\")");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_concat_avs_preserves_order() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("ordered.avs");
        let paths: Vec<&Path> = vec![
            Path::new("/z.avs"),
            Path::new("/a.avs"),
            Path::new("/m.avs"),
        ];

        // Act
        concat(&output, &paths).unwrap();

        // Assert
        let content = std::fs::read_to_string(&output).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[0], "Import(\"/z.avs\")");
        assert_eq!(lines[1], "Import(\"/a.avs\")");
        assert_eq!(lines[2], "Import(\"/m.avs\")");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_create_cutcm() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("in_cutcm.avs");

        // Act
        create_cutcm(
            &output,
            Path::new("/out/in_org.avs"),
            Path::new("/out/obs_cut.avs"),
        )
        .unwrap();

        // Assert
        let content = std::fs::read_to_string(&output).unwrap();
        assert_eq!(content.lines().count(), 2);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_create_cutcm_logo() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("in_cutcm_logo.avs");

        // Act
        create_cutcm_logo(
            &output,
            Path::new("/out/in_org.avs"),
            Path::new("/out/obs_logo_erase.avs"),
            Path::new("/out/obs_cut.avs"),
        )
        .unwrap();

        // Assert
        let content = std::fs::read_to_string(&output).unwrap();
        assert_eq!(content.lines().count(), 3);
    }
}
