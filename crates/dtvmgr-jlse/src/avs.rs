//! Input AVS template generation.
//!
//! Generates an AviSynth script that loads a TS file via `LWLibavSource`.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};

/// Default stream index for audio.
pub const STREAM_INDEX_NORMAL: i32 = 1;

/// Create an input AVS script that loads `input_file` with the given
/// `stream_index`.
///
/// The generated script uses `LWLibavVideoSource` and
/// `LWLibavAudioSource` to load the TS file.
///
/// # Errors
///
/// Returns an error if the output file cannot be written.
pub fn create(output_path: &Path, input_file: &Path, stream_index: i32) -> Result<()> {
    let ts_path = input_file.display();

    let mut content = String::new();
    let _ = writeln!(content, "TSFilePath=\"{ts_path}\"");
    let _ = writeln!(
        content,
        "LWLibavVideoSource(TSFilePath, repeat=true, dominance=1)"
    );
    let _ = writeln!(
        content,
        "AudioDub(last, LWLibavAudioSource(TSFilePath, stream_index={stream_index}, av_sync=true))"
    );

    std::fs::write(output_path, &content)
        .with_context(|| format!("failed to write AVS file: {}", output_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_create_normal_stream_index() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("in_org.avs");
        let input = Path::new("/rec/video.ts");

        // Act
        create(&output, input, STREAM_INDEX_NORMAL).unwrap();

        // Assert
        let content = std::fs::read_to_string(&output).unwrap();
        assert!(content.contains("TSFilePath=\"/rec/video.ts\""));
        assert!(content.contains("LWLibavVideoSource(TSFilePath, repeat=true, dominance=1)"));
        assert!(content.contains("stream_index=1"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_create_file_written() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("test.avs");

        // Act
        create(&output, Path::new("/input.ts"), 1).unwrap();

        // Assert
        assert!(output.exists());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_create_content_has_three_lines() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("test.avs");

        // Act
        create(&output, Path::new("/input.ts"), 1).unwrap();

        // Assert
        let content = std::fs::read_to_string(&output).unwrap();
        assert_eq!(content.lines().count(), 3);
    }
}
