//! Wrapper for the `ffmpeg` external command.
//!
//! Encodes media files with optional chapter metadata injection.

use std::ffi::OsStr;
use std::path::Path;

use anyhow::Result;

// ── Types ────────────────────────────────────────────────────

/// Metadata fields for the encoded file.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FfmpegMetadata {
    /// Title (e.g. programme name).
    pub title: String,
    /// Description text.
    pub description: String,
    /// Extended description text.
    pub extended: String,
}

// ── Functions ────────────────────────────────────────────────

/// Run ffmpeg encoding with the given parameters.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits
/// with a non-zero status code.
pub fn run(
    binary: &Path,
    avs_file: &Path,
    output_file: &Path,
    chapter_file: Option<&Path>,
    metadata: &FfmpegMetadata,
    extra_options: &str,
) -> Result<()> {
    let args = build_args(avs_file, output_file, chapter_file, metadata, extra_options);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    super::run(binary, &os_args)
}

/// Build the argument list for ffmpeg.
///
/// Format:
/// ```text
/// ffmpeg -hide_banner -y -ignore_unknown -i <avs_file>
///        [-i <chapter> -map_metadata 1 -metadata title=...
///         -metadata comment=... -movflags +use_metadata_tags]
///        [extra_options...]
///        <output.mp4>
/// ```
#[must_use]
pub fn build_args(
    avs_file: &Path,
    output_file: &Path,
    chapter_file: Option<&Path>,
    metadata: &FfmpegMetadata,
    extra_options: &str,
) -> Vec<String> {
    let mut args = vec![
        "-hide_banner".to_owned(),
        "-y".to_owned(),
        "-ignore_unknown".to_owned(),
        "-i".to_owned(),
        avs_file.display().to_string(),
    ];

    if let Some(chapter) = chapter_file {
        args.push("-i".to_owned());
        args.push(chapter.display().to_string());
        args.push("-map_metadata".to_owned());
        args.push("1".to_owned());

        if !metadata.title.is_empty() {
            args.push("-metadata".to_owned());
            args.push(format!("title={}", metadata.title));
        }

        let comment = build_comment(metadata);
        if !comment.is_empty() {
            args.push("-metadata".to_owned());
            args.push(format!("comment={comment}"));
        }

        args.push("-movflags".to_owned());
        args.push("+use_metadata_tags".to_owned());
    }

    for opt in extra_options.split_whitespace() {
        args.push(opt.to_owned());
    }

    args.push(output_file.display().to_string());

    args
}

/// Build comment string from description and extended fields.
#[must_use]
pub fn build_comment(metadata: &FfmpegMetadata) -> String {
    let mut parts: Vec<&str> = Vec::new();

    if !metadata.description.is_empty() {
        parts.push(&metadata.description);
    }
    if !metadata.extended.is_empty() {
        parts.push(&metadata.extended);
    }

    parts.join("\n")
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use std::path::Path;

    use super::*;

    // ── build_args ──────────────────────────────────────────

    #[test]
    fn test_build_args_no_chapter() {
        // Arrange
        let metadata = FfmpegMetadata::default();

        // Act
        let args = build_args(
            Path::new("/out/in_cutcm.avs"),
            Path::new("/enc/output.mp4"),
            None,
            &metadata,
            "",
        );

        // Assert
        assert_eq!(args[0], "-hide_banner");
        assert_eq!(args[1], "-y");
        assert_eq!(args[2], "-ignore_unknown");
        assert_eq!(args[3], "-i");
        assert_eq!(args[4], "/out/in_cutcm.avs");
        assert_eq!(args[5], "/enc/output.mp4");
        assert_eq!(args.len(), 6);
    }

    #[test]
    fn test_build_args_with_chapter() {
        // Arrange
        let metadata = FfmpegMetadata {
            title: "Test Show".to_owned(),
            description: "Episode 1".to_owned(),
            extended: "Extended info".to_owned(),
        };

        // Act
        let args = build_args(
            Path::new("/out/in_cutcm.avs"),
            Path::new("/enc/output.mp4"),
            Some(Path::new("/out/chapter.txt")),
            &metadata,
            "",
        );

        // Assert
        assert_eq!(args[3], "-i");
        assert_eq!(args[4], "/out/in_cutcm.avs");
        assert_eq!(args[5], "-i");
        assert_eq!(args[6], "/out/chapter.txt");
        assert_eq!(args[7], "-map_metadata");
        assert_eq!(args[8], "1");
        assert_eq!(args[9], "-metadata");
        assert_eq!(args[10], "title=Test Show");
        assert_eq!(args[11], "-metadata");
        assert!(args[12].starts_with("comment=Episode 1"));
        assert_eq!(args[13], "-movflags");
        assert_eq!(args[14], "+use_metadata_tags");
        // Last element is the output file
        assert_eq!(*args.last().unwrap(), "/enc/output.mp4");
    }

    #[test]
    fn test_build_args_with_extra_options() {
        // Arrange
        let metadata = FfmpegMetadata::default();

        // Act
        let args = build_args(
            Path::new("/out/in_cutcm.avs"),
            Path::new("/enc/output.mp4"),
            None,
            &metadata,
            "-c:v libx264 -crf 23",
        );

        // Assert — extra options before output file
        let output_idx = args.len() - 1;
        assert_eq!(args[output_idx], "/enc/output.mp4");
        assert!(args.contains(&"-c:v".to_owned()));
        assert!(args.contains(&"libx264".to_owned()));
        assert!(args.contains(&"-crf".to_owned()));
        assert!(args.contains(&"23".to_owned()));
    }

    #[test]
    fn test_build_args_chapter_no_title() {
        // Arrange
        let metadata = FfmpegMetadata {
            title: String::new(),
            description: "Desc".to_owned(),
            extended: String::new(),
        };

        // Act
        let args = build_args(
            Path::new("/out/in.avs"),
            Path::new("/out.mp4"),
            Some(Path::new("/chap.txt")),
            &metadata,
            "",
        );

        // Assert — no title= metadata, but comment= present
        assert!(!args.iter().any(|a| a.starts_with("title=")));
        assert!(args.iter().any(|a| a == "comment=Desc"));
    }

    // ── build_comment ───────────────────────────────────────

    #[test]
    fn test_build_comment_both() {
        // Arrange
        let metadata = FfmpegMetadata {
            title: String::new(),
            description: "Desc".to_owned(),
            extended: "Ext".to_owned(),
        };

        // Act / Assert
        assert_eq!(build_comment(&metadata), "Desc\nExt");
    }

    #[test]
    fn test_build_comment_description_only() {
        // Arrange
        let metadata = FfmpegMetadata {
            title: String::new(),
            description: "Desc".to_owned(),
            extended: String::new(),
        };

        // Act / Assert
        assert_eq!(build_comment(&metadata), "Desc");
    }

    #[test]
    fn test_build_comment_extended_only() {
        // Arrange
        let metadata = FfmpegMetadata {
            title: String::new(),
            description: String::new(),
            extended: "Ext".to_owned(),
        };

        // Act / Assert
        assert_eq!(build_comment(&metadata), "Ext");
    }

    #[test]
    fn test_build_comment_empty() {
        // Arrange
        let metadata = FfmpegMetadata::default();

        // Act / Assert
        assert!(build_comment(&metadata).is_empty());
    }
}
