//! Output paths, binary paths, and data paths for pipeline execution.
//!
//! This module manages all file paths used during a single CM detection
//! pipeline run, including output files, external binary locations, and
//! CSV data file paths.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::debug;

use crate::types::JlseConfig;

/// All output file paths for a single processing run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputPaths {
    /// Base output directory: `<result_dir>/<filename>/`
    pub save_dir: PathBuf,
    /// Input AVS file: `in_org.avs`
    pub input_avs: PathBuf,
    /// `chapter_exe` output: `obs_chapterexe.txt`
    pub chapterexe_output: PathBuf,
    /// logoframe text output: `obs_logoframe.txt`
    pub logoframe_txt_output: PathBuf,
    /// logoframe AVS output: `obs_logo_erase.avs`
    pub logoframe_avs_output: PathBuf,
    /// Merged parameter info: `obs_param.txt`
    pub obs_param_path: PathBuf,
    /// `join_logo_scp` structure output: `obs_jlscp.txt`
    pub jlscp_output: PathBuf,
    /// Cut AVS (Trim commands): `obs_cut.avs`
    pub output_avs_cut: PathBuf,
    /// Concatenated cut AVS: `in_cutcm.avs`
    pub output_avs_in_cut: PathBuf,
    /// Concatenated cut+logo AVS: `in_cutcm_logo.avs`
    pub output_avs_in_cut_logo: PathBuf,
    /// `FFmpeg` filter output: `ffmpeg.filter`
    pub output_filter_cut: PathBuf,
    /// Chapter ORG (all sections): `obs_chapter_org.chapter.txt`
    pub file_txt_cpt_org: PathBuf,
    /// Chapter CUT (non-cut only): `obs_chapter_cut.chapter.txt`
    pub file_txt_cpt_cut: PathBuf,
    /// Chapter `TVTPlay` format: `obs_chapter_tvtplay.chapter`
    pub file_txt_cpt_tvt: PathBuf,
}

/// Paths to external binary commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryPaths {
    /// logoframe binary: `<jl_dir>/../bin/logoframe`
    pub logoframe: PathBuf,
    /// `chapter_exe` binary: `<jl_dir>/../bin/chapter_exe`
    pub chapter_exe: PathBuf,
    /// `join_logo_scp` binary: `<jl_dir>/../bin/join_logo_scp`
    pub join_logo_scp: PathBuf,
    /// ffprobe binary: `/usr/local/bin/ffprobe`
    pub ffprobe: PathBuf,
    /// ffmpeg binary: `/usr/local/bin/ffmpeg`
    pub ffmpeg: PathBuf,
    /// tstables binary (resolved from PATH by default)
    pub tstables: PathBuf,
}

/// Paths to CSV data files under `<jl_dir>/data/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPaths {
    /// Channel list CSV: `<jl_dir>/data/ChList.csv`
    pub channel_list: PathBuf,
    /// JL parameter list 1: `<jl_dir>/data/ChParamJL1.csv`
    pub param_jl1: PathBuf,
    /// JL parameter list 2: `<jl_dir>/data/ChParamJL2.csv`
    pub param_jl2: PathBuf,
}

/// Create the output directory and build all 15 output file paths.
///
/// Creates `<result_dir>/<filename>/` via `create_dir_all` and returns an
/// [`OutputPaths`] with every field pointing into that directory.
///
/// # Errors
///
/// Returns an error if the output directory cannot be created.
pub fn init_output_paths(result_dir: &Path, filename: &str) -> Result<OutputPaths> {
    let save_dir = result_dir.join(filename);
    std::fs::create_dir_all(&save_dir)
        .with_context(|| format!("failed to create output dir: {}", save_dir.display()))?;

    debug!(dir = %save_dir.display(), "created output directory");

    Ok(OutputPaths {
        input_avs: save_dir.join("in_org.avs"),
        chapterexe_output: save_dir.join("obs_chapterexe.txt"),
        logoframe_txt_output: save_dir.join("obs_logoframe.txt"),
        logoframe_avs_output: save_dir.join("obs_logo_erase.avs"),
        obs_param_path: save_dir.join("obs_param.txt"),
        jlscp_output: save_dir.join("obs_jlscp.txt"),
        output_avs_cut: save_dir.join("obs_cut.avs"),
        output_avs_in_cut: save_dir.join("in_cutcm.avs"),
        output_avs_in_cut_logo: save_dir.join("in_cutcm_logo.avs"),
        output_filter_cut: save_dir.join("ffmpeg.filter"),
        file_txt_cpt_org: save_dir.join("obs_chapter_org.chapter.txt"),
        file_txt_cpt_cut: save_dir.join("obs_chapter_cut.chapter.txt"),
        file_txt_cpt_tvt: save_dir.join("obs_chapter_tvtplay.chapter"),
        save_dir,
    })
}

impl BinaryPaths {
    /// Derive binary paths from the given [`JlseConfig`].
    ///
    /// JL-bundled binaries are resolved relative to `dirs.jl`'s parent
    /// directory under `bin/`. System binaries default to `/usr/local/bin/`.
    /// Any field present in `config.bins` overrides the default.
    #[must_use]
    pub fn from_config(config: &JlseConfig) -> Self {
        let bin_dir = config.dirs.bin_dir();
        let bins = &config.bins;
        Self {
            logoframe: bins
                .logoframe
                .clone()
                .unwrap_or_else(|| bin_dir.join("logoframe")),
            chapter_exe: bins
                .chapter_exe
                .clone()
                .unwrap_or_else(|| bin_dir.join("chapter_exe")),
            join_logo_scp: bins
                .join_logo_scp
                .clone()
                .unwrap_or_else(|| bin_dir.join("join_logo_scp")),
            ffprobe: bins
                .ffprobe
                .clone()
                .unwrap_or_else(|| PathBuf::from("/usr/local/bin/ffprobe")),
            ffmpeg: bins
                .ffmpeg
                .clone()
                .unwrap_or_else(|| PathBuf::from("/usr/local/bin/ffmpeg")),
            tstables: bins
                .tstables
                .clone()
                .unwrap_or_else(|| PathBuf::from("tstables")),
        }
    }
}

impl DataPaths {
    /// Derive CSV data file paths from the given [`JlseConfig`].
    ///
    /// All paths are under `<dirs.jl>/data/`.
    #[must_use]
    pub fn from_config(config: &JlseConfig) -> Self {
        let data_dir = config.dirs.jl.join("data");

        Self {
            channel_list: data_dir.join("ChList.csv"),
            param_jl1: data_dir.join("ChParamJL1.csv"),
            param_jl2: data_dir.join("ChParamJL2.csv"),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    use crate::types::{JlseBins, JlseDirs};

    fn sample_config(jl: PathBuf) -> JlseConfig {
        JlseConfig {
            dirs: JlseDirs {
                jl,
                logo: PathBuf::from("/tmp/logo"),
                result: PathBuf::from("/tmp/result"),
            },
            bins: JlseBins::default(),
            encode: None,
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // tempdir requires mkdir, unsupported under Miri isolation
    fn test_init_output_paths_creates_directory() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let result_dir = tmp.path();

        // Act
        let paths = init_output_paths(result_dir, "test_file").unwrap();

        // Assert
        assert!(paths.save_dir.is_dir());
    }

    #[test]
    #[cfg_attr(miri, ignore)] // tempdir requires mkdir, unsupported under Miri isolation
    fn test_init_output_paths_all_paths_under_save_dir() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();

        // Act
        let paths = init_output_paths(tmp.path(), "show").unwrap();

        // Assert
        let all_paths = [
            &paths.input_avs,
            &paths.chapterexe_output,
            &paths.logoframe_txt_output,
            &paths.logoframe_avs_output,
            &paths.obs_param_path,
            &paths.jlscp_output,
            &paths.output_avs_cut,
            &paths.output_avs_in_cut,
            &paths.output_avs_in_cut_logo,
            &paths.output_filter_cut,
            &paths.file_txt_cpt_org,
            &paths.file_txt_cpt_cut,
            &paths.file_txt_cpt_tvt,
        ];
        for p in all_paths {
            assert!(
                p.starts_with(&paths.save_dir),
                "{} is not under {}",
                p.display(),
                paths.save_dir.display()
            );
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // tempdir requires mkdir, unsupported under Miri isolation
    fn test_init_output_paths_exact_filenames() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();

        // Act
        let paths = init_output_paths(tmp.path(), "rec").unwrap();

        // Assert
        let expected = [
            (&paths.input_avs, "in_org.avs"),
            (&paths.chapterexe_output, "obs_chapterexe.txt"),
            (&paths.logoframe_txt_output, "obs_logoframe.txt"),
            (&paths.logoframe_avs_output, "obs_logo_erase.avs"),
            (&paths.obs_param_path, "obs_param.txt"),
            (&paths.jlscp_output, "obs_jlscp.txt"),
            (&paths.output_avs_cut, "obs_cut.avs"),
            (&paths.output_avs_in_cut, "in_cutcm.avs"),
            (&paths.output_avs_in_cut_logo, "in_cutcm_logo.avs"),
            (&paths.output_filter_cut, "ffmpeg.filter"),
            (&paths.file_txt_cpt_org, "obs_chapter_org.chapter.txt"),
            (&paths.file_txt_cpt_cut, "obs_chapter_cut.chapter.txt"),
            (&paths.file_txt_cpt_tvt, "obs_chapter_tvtplay.chapter"),
        ];
        for (path, name) in expected {
            assert_eq!(
                path.file_name().unwrap().to_str().unwrap(),
                name,
                "unexpected filename for {}",
                path.display()
            );
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // tempdir requires mkdir, unsupported under Miri isolation
    fn test_init_output_paths_nested_directory_creation() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let deep = tmp.path().join("a").join("b").join("c");

        // Act
        let paths = init_output_paths(&deep, "nested").unwrap();

        // Assert
        assert!(paths.save_dir.is_dir());
    }

    #[test]
    fn test_binary_paths_from_config() {
        // Arrange — JlseBins::default() provides standard paths
        let config = sample_config(PathBuf::from("/opt/module/JL"));

        // Act
        let bins = BinaryPaths::from_config(&config);

        // Assert — defaults from JlseBins::default()
        assert_eq!(
            bins.logoframe,
            PathBuf::from("/join_logo_scp_trial/bin/logoframe")
        );
        assert_eq!(
            bins.chapter_exe,
            PathBuf::from("/join_logo_scp_trial/bin/chapter_exe")
        );
        assert_eq!(
            bins.join_logo_scp,
            PathBuf::from("/join_logo_scp_trial/bin/join_logo_scp")
        );
        assert_eq!(bins.ffprobe, PathBuf::from("/opt/ffmpeg/bin/ffprobe"));
        assert_eq!(bins.ffmpeg, PathBuf::from("/opt/ffmpeg/bin/ffmpeg"));
    }

    #[test]
    fn test_binary_paths_none_falls_back_to_bin_dir() {
        // Arrange — explicitly set all bins to None to test fallback
        let mut config = sample_config(PathBuf::from("/srv/recmgr/JL"));
        config.bins = JlseBins {
            logoframe: None,
            chapter_exe: None,
            join_logo_scp: None,
            ffmpeg: None,
            ffprobe: None,
            tstables: None,
        };

        // Act
        let bins = BinaryPaths::from_config(&config);

        // Assert — falls back to bin_dir derivation
        assert_eq!(bins.logoframe, PathBuf::from("/srv/recmgr/bin/logoframe"));
        assert_eq!(
            bins.chapter_exe,
            PathBuf::from("/srv/recmgr/bin/chapter_exe")
        );
    }

    #[test]
    fn test_data_paths_from_config() {
        // Arrange
        let config = sample_config(PathBuf::from("/opt/module/JL"));

        // Act
        let data = DataPaths::from_config(&config);

        // Assert
        assert_eq!(
            data.channel_list,
            PathBuf::from("/opt/module/JL/data/ChList.csv")
        );
        assert_eq!(
            data.param_jl1,
            PathBuf::from("/opt/module/JL/data/ChParamJL1.csv")
        );
        assert_eq!(
            data.param_jl2,
            PathBuf::from("/opt/module/JL/data/ChParamJL2.csv")
        );
    }

    #[test]
    fn test_binary_paths_with_defaults() {
        // Arrange — JlseBins::default() provides standard paths
        let config = sample_config(PathBuf::from("/opt/module/JL"));

        // Act
        let bins = BinaryPaths::from_config(&config);

        // Assert — defaults from JlseBins::default()
        assert_eq!(
            bins.logoframe,
            PathBuf::from("/join_logo_scp_trial/bin/logoframe")
        );
        assert_eq!(
            bins.chapter_exe,
            PathBuf::from("/join_logo_scp_trial/bin/chapter_exe")
        );
        assert_eq!(
            bins.join_logo_scp,
            PathBuf::from("/join_logo_scp_trial/bin/join_logo_scp")
        );
        assert_eq!(bins.ffprobe, PathBuf::from("/opt/ffmpeg/bin/ffprobe"));
        assert_eq!(bins.ffmpeg, PathBuf::from("/opt/ffmpeg/bin/ffmpeg"));
    }

    #[test]
    fn test_binary_paths_single_override() {
        // Arrange
        let mut config = sample_config(PathBuf::from("/opt/module/JL"));
        config.bins.ffmpeg = Some(PathBuf::from("/usr/bin/ffmpeg"));

        // Act
        let bins = BinaryPaths::from_config(&config);

        // Assert — only ffmpeg overridden, rest from JlseBins::default()
        assert_eq!(bins.ffmpeg, PathBuf::from("/usr/bin/ffmpeg"));
        assert_eq!(bins.ffprobe, PathBuf::from("/opt/ffmpeg/bin/ffprobe"));
        assert_eq!(
            bins.logoframe,
            PathBuf::from("/join_logo_scp_trial/bin/logoframe")
        );
    }

    #[test]
    fn test_binary_paths_all_overrides() {
        // Arrange
        let mut config = sample_config(PathBuf::from("/opt/module/JL"));
        config.bins = JlseBins {
            logoframe: Some(PathBuf::from("/custom/logoframe")),
            chapter_exe: Some(PathBuf::from("/custom/chapter_exe")),
            join_logo_scp: Some(PathBuf::from("/custom/join_logo_scp")),
            ffprobe: Some(PathBuf::from("/custom/ffprobe")),
            ffmpeg: Some(PathBuf::from("/custom/ffmpeg")),
            tstables: Some(PathBuf::from("/custom/tstables")),
        };

        // Act
        let bins = BinaryPaths::from_config(&config);

        // Assert — all overridden
        assert_eq!(bins.logoframe, PathBuf::from("/custom/logoframe"));
        assert_eq!(bins.chapter_exe, PathBuf::from("/custom/chapter_exe"));
        assert_eq!(bins.join_logo_scp, PathBuf::from("/custom/join_logo_scp"));
        assert_eq!(bins.ffprobe, PathBuf::from("/custom/ffprobe"));
        assert_eq!(bins.ffmpeg, PathBuf::from("/custom/ffmpeg"));
        assert_eq!(bins.tstables, PathBuf::from("/custom/tstables"));
    }
}
