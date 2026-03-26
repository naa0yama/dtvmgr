//! Input AVS template generation.
//!
//! Generates an AviSynth script that loads a TS file via `LWLibavSource`.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use tracing::instrument;

/// Default stream index for audio.
pub const STREAM_INDEX_NORMAL: u32 = 1;

/// Create an input AVS script that loads `input_file` with the given
/// audio `stream_index`.
///
/// The generated script uses `LWLibavVideoSource` and
/// `LWLibavAudioSource` to load the TS file.
///
/// # Errors
///
/// Returns an error if the output file cannot be written.
#[instrument(skip_all, err(level = "error"))]
pub fn create(output_path: &Path, input_file: &Path, stream_index: u32) -> Result<()> {
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
    fn test_create_with_detected_index() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("in_org.avs");
        let input = Path::new("/rec/video.ts");

        // Act — use index 3 (detected by ffprobe)
        create(&output, input, 3).unwrap();

        // Assert
        let content = std::fs::read_to_string(&output).unwrap();
        assert!(content.contains("TSFilePath=\"/rec/video.ts\""));
        assert!(content.contains("LWLibavVideoSource(TSFilePath, repeat=true, dominance=1)"));
        assert!(content.contains("stream_index=3"));
        assert_eq!(content.lines().count(), 3);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_create_with_default_index() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("in_org.avs");

        // Act — fallback to STREAM_INDEX_NORMAL
        create(&output, Path::new("/input.ts"), STREAM_INDEX_NORMAL).unwrap();

        // Assert
        let content = std::fs::read_to_string(&output).unwrap();
        assert!(content.contains("stream_index=1"));
        assert_eq!(content.lines().count(), 3);
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
}
