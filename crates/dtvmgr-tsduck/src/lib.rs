//! `TSDuck` command wrappers and EIT parser for MPEG-TS files.

use std::path::Path;

use anyhow::{Context, Result};

/// External command wrappers for `TSDuck` tools.
pub mod command;
/// EIT (Event Information Table) XML parser.
pub mod eit;
/// PAT (Program Association Table) XML parser.
pub mod pat;
/// TS file seek and chunk extraction.
pub mod seek;

/// Detect the recording target from the middle of a TS file.
///
/// Extracts a chunk from the file's midpoint, runs `tstables` to parse
/// EIT p/f tables, and identifies the recording target.
///
/// Returns the detected [`eit::RecordingTarget`] (if any) and the raw
/// EIT XML string for further use (e.g. saving as attachment).
///
/// # Errors
///
/// Returns an error if chunk extraction, `tstables` execution, or XML
/// parsing fails.
pub fn detect_target_from_middle(
    tstables_bin: &Path,
    input: &Path,
) -> Result<(Option<eit::RecordingTarget>, String)> {
    let chunk = seek::extract_middle_chunk(input, seek::DEFAULT_CHUNK_SIZE)
        .context("failed to extract middle chunk")?;

    let xml = command::extract_eit_from_chunk(tstables_bin, &chunk)
        .context("failed to extract EIT p/f from chunk")?;

    let programs = eit::parse_eit_xml(&xml).context("failed to parse mid-file EIT XML")?;

    let target = eit::detect_recording_target(&programs);

    Ok((target, xml))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::Path;

    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn detect_target_from_middle_nonexistent_input_fails() {
        // Arrange
        let tstables = Path::new("/usr/bin/tstables");
        let input = Path::new("/nonexistent/file.ts");

        // Act
        let result = detect_target_from_middle(tstables, input);

        // Assert: should fail at chunk extraction
        let err = result.unwrap_err();
        assert!(
            format!("{err:?}").contains("extract middle chunk"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn detect_target_from_middle_empty_file_fails() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("empty.ts");
        std::fs::write(&input, b"").unwrap();
        let tstables = Path::new("/usr/bin/tstables");

        // Act
        let result = detect_target_from_middle(tstables, &input);

        // Assert: should fail because file is too small for chunk extraction
        assert!(result.is_err());
    }
}
