//! Wrapper for the `ffmpeg` external command.
//!
//! Encodes media files into MKV with optional chapter metadata and
//! EIT XML attachment.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::Result;

// ── Types ────────────────────────────────────────────────────

/// Metadata fields for the encoded MKV file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MkvMetadata {
    /// `TITLE` tag (`program_name` from `short_event_descriptor`).
    pub title: Option<String>,
    /// `SUBTITLE` tag (`text` from `short_event_descriptor`).
    pub subtitle: Option<String>,
    /// `DESCRIPTION` tag (concatenated extended event text).
    pub description: Option<String>,
    /// `GENRE` tag (ARIB genre name in English).
    pub genre: Option<String>,
    /// `DATE_RECORDED` tag (event `start_time`).
    pub date_recorded: Option<String>,
    /// Path to EIT XML file for attachment.
    pub eit_xml_path: Option<PathBuf>,
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
    metadata: &MkvMetadata,
    extra_options: &str,
) -> Result<()> {
    let args = build_args(avs_file, output_file, chapter_file, metadata, extra_options);
    let os_args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    super::run(binary, &os_args)
}

/// Build the argument list for ffmpeg (MKV output).
///
/// Format:
/// ```text
/// ffmpeg -hide_banner -y -ignore_unknown -i <avs_file>
///        [-i <chapter> -map_metadata 1]
///        [-metadata TITLE=... -metadata SUBTITLE=... -metadata GENRE=...
///         -metadata DESCRIPTION=... -metadata DATE_RECORDED=...]
///        [-attach eit.xml -metadata:s:t:0 mimetype=application/xml
///         -metadata:s:t:0 filename=eit.xml]
///        [extra_options...]
///        <output.mkv>
/// ```
#[must_use]
pub fn build_args(
    avs_file: &Path,
    output_file: &Path,
    chapter_file: Option<&Path>,
    metadata: &MkvMetadata,
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
    }

    // Standard MKV metadata tags
    push_metadata_tag(&mut args, "TITLE", metadata.title.as_deref());
    push_metadata_tag(&mut args, "SUBTITLE", metadata.subtitle.as_deref());
    push_metadata_tag(&mut args, "DESCRIPTION", metadata.description.as_deref());
    push_metadata_tag(&mut args, "GENRE", metadata.genre.as_deref());
    push_metadata_tag(
        &mut args,
        "DATE_RECORDED",
        metadata.date_recorded.as_deref(),
    );

    // EIT XML attachment
    if let Some(ref xml_path) = metadata.eit_xml_path {
        args.push("-attach".to_owned());
        args.push(xml_path.display().to_string());
        args.push("-metadata:s:t:0".to_owned());
        args.push("mimetype=application/xml".to_owned());
        args.push("-metadata:s:t:0".to_owned());
        args.push("filename=eit.xml".to_owned());
    }

    for opt in extra_options.split_whitespace() {
        args.push(opt.to_owned());
    }

    args.push(output_file.display().to_string());

    args
}

/// Push a `-metadata TAG=value` pair if value is non-empty.
fn push_metadata_tag(args: &mut Vec<String>, tag: &str, value: Option<&str>) {
    if let Some(v) = value
        && !v.is_empty()
    {
        args.push("-metadata".to_owned());
        args.push(format!("{tag}={v}"));
    }
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use std::path::{Path, PathBuf};

    use super::*;

    // ── build_args ──────────────────────────────────────────

    #[test]
    fn test_build_args_no_chapter_no_metadata() {
        // Arrange
        let metadata = MkvMetadata::default();

        // Act
        let args = build_args(
            Path::new("/out/in_cutcm.avs"),
            Path::new("/enc/output.mkv"),
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
        assert_eq!(args[5], "/enc/output.mkv");
        assert_eq!(args.len(), 6);
    }

    #[test]
    fn test_build_args_with_chapter_and_metadata() {
        // Arrange
        let metadata = MkvMetadata {
            title: Some("Test Show".to_owned()),
            subtitle: Some("Episode 1".to_owned()),
            description: Some("A test description".to_owned()),
            genre: Some("Animation/Special Effects".to_owned()),
            date_recorded: Some("2024-12-31 15:00:00".to_owned()),
            eit_xml_path: None,
        };

        // Act
        let args = build_args(
            Path::new("/out/in_cutcm.avs"),
            Path::new("/enc/output.mkv"),
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
        assert_eq!(args[10], "TITLE=Test Show");
        assert_eq!(args[11], "-metadata");
        assert_eq!(args[12], "SUBTITLE=Episode 1");
        assert_eq!(args[13], "-metadata");
        assert_eq!(args[14], "DESCRIPTION=A test description");
        assert_eq!(args[15], "-metadata");
        assert_eq!(args[16], "GENRE=Animation/Special Effects");
        assert_eq!(args[17], "-metadata");
        assert_eq!(args[18], "DATE_RECORDED=2024-12-31 15:00:00");
        assert_eq!(*args.last().unwrap(), "/enc/output.mkv");
    }

    #[test]
    fn test_build_args_with_eit_xml_attachment() {
        // Arrange
        let metadata = MkvMetadata {
            title: Some("Show".to_owned()),
            eit_xml_path: Some(PathBuf::from("/out/eit.xml")),
            ..MkvMetadata::default()
        };

        // Act
        let args = build_args(
            Path::new("/out/in_cutcm.avs"),
            Path::new("/enc/output.mkv"),
            None,
            &metadata,
            "",
        );

        // Assert
        assert!(args.contains(&"-attach".to_owned()));
        assert!(args.contains(&"/out/eit.xml".to_owned()));
        assert!(args.contains(&"-metadata:s:t:0".to_owned()));
        assert!(args.contains(&"mimetype=application/xml".to_owned()));
        assert!(args.contains(&"filename=eit.xml".to_owned()));
    }

    #[test]
    fn test_build_args_with_extra_options() {
        // Arrange
        let metadata = MkvMetadata::default();

        // Act
        let args = build_args(
            Path::new("/out/in_cutcm.avs"),
            Path::new("/enc/output.mkv"),
            None,
            &metadata,
            "-c:v libx264 -crf 23",
        );

        // Assert — extra options before output file
        let output_idx = args.len() - 1;
        assert_eq!(args[output_idx], "/enc/output.mkv");
        assert!(args.contains(&"-c:v".to_owned()));
        assert!(args.contains(&"libx264".to_owned()));
        assert!(args.contains(&"-crf".to_owned()));
        assert!(args.contains(&"23".to_owned()));
    }

    #[test]
    fn test_build_args_chapter_no_title() {
        // Arrange
        let metadata = MkvMetadata {
            description: Some("Desc".to_owned()),
            ..MkvMetadata::default()
        };

        // Act
        let args = build_args(
            Path::new("/out/in.avs"),
            Path::new("/out.mkv"),
            Some(Path::new("/chap.txt")),
            &metadata,
            "",
        );

        // Assert — no TITLE metadata, but DESCRIPTION present
        assert!(!args.iter().any(|a| a.starts_with("TITLE=")));
        assert!(args.iter().any(|a| a == "DESCRIPTION=Desc"));
    }

    #[test]
    fn test_build_args_empty_string_metadata_skipped() {
        // Arrange
        let metadata = MkvMetadata {
            title: Some(String::new()),
            subtitle: Some("Sub".to_owned()),
            ..MkvMetadata::default()
        };

        // Act
        let args = build_args(
            Path::new("/out/in.avs"),
            Path::new("/out.mkv"),
            None,
            &metadata,
            "",
        );

        // Assert — empty title is skipped, subtitle is present
        assert!(!args.iter().any(|a| a.starts_with("TITLE=")));
        assert!(args.iter().any(|a| a == "SUBTITLE=Sub"));
    }

    #[test]
    fn test_build_args_no_movflags() {
        // Arrange — MKV does not use movflags
        let metadata = MkvMetadata {
            title: Some("Show".to_owned()),
            ..MkvMetadata::default()
        };

        // Act
        let args = build_args(
            Path::new("/out/in.avs"),
            Path::new("/out.mkv"),
            Some(Path::new("/chap.txt")),
            &metadata,
            "",
        );

        // Assert
        assert!(!args.iter().any(|a| a.contains("movflags")));
    }
}
