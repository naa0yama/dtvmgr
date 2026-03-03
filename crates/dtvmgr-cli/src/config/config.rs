//! `AppConfig` struct and TOML read/write.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use dtvmgr_jlse::types::JlseConfig;
use serde::{Deserialize, Serialize};

/// Top-level application configuration.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AppConfig {
    /// Syoboi Calendar settings.
    #[serde(default)]
    pub syoboi: SyoboiConfig,
    /// TMDB settings.
    #[serde(default)]
    pub tmdb: TmdbConfig,
    /// Normalize viewer settings.
    #[serde(default)]
    pub normalize: NormalizeConfig,
    /// CM detection pipeline settings.
    #[serde(default)]
    pub jlse: Option<JlseConfig>,
}

/// Syoboi Calendar settings.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SyoboiConfig {
    /// Channel selection settings.
    #[serde(default)]
    pub channels: ChannelsConfig,
    /// Title settings.
    #[serde(default)]
    pub titles: TitlesConfig,
}

/// Default category codes to include.
fn default_cat() -> Vec<u32> {
    vec![1, 7, 8, 10]
}

/// Default movie category codes.
fn default_cat_movie() -> Vec<u32> {
    vec![8]
}

/// Default TIDs excluded from display in the title viewer.
fn default_excludes() -> Vec<u32> {
    vec![
        5, 44, 46, 92, 93, 385, 399, 414, 438, 464, 604, 620, 635, 679, 700, 706, 727, 811, 842,
        855, 868, 871, 894, 913, 967, 1003, 1112, 1192, 1235, 1245, 1363, 1387, 1447, 1511, 1512,
        1640, 1647, 1726, 1764, 1775, 1786, 1803, 1829, 1850, 1865, 1910, 2102, 2103, 2179, 2181,
        2232, 2300, 2319, 2415, 2440, 2710, 2871, 2950, 2956, 2976, 3007, 3154, 3291, 3350, 3384,
        3407, 3476, 3519, 3520, 3533, 3547, 3721, 3732, 3880, 3908, 3940, 4062, 4202, 4243, 4272,
        4290, 4317, 4349, 4357, 4391, 4397, 4457, 4459, 4534, 4564, 4599, 4608, 4653, 4720, 4744,
        4812, 4879, 4909, 5083, 5104, 5109, 5173, 5189, 5268, 5301, 5318, 5405, 5446, 5478, 5486,
        5645, 5727, 5771, 5807, 5987, 6019, 6122, 6169, 6347, 6496, 6507, 6514, 6605, 6696, 6698,
        6984, 6993, 7059, 7264, 7345, 7537, 7586, 7605, 7617,
    ]
}

/// Title configuration.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TitlesConfig {
    /// Syoboi category codes to include during sync.
    #[serde(default = "default_cat")]
    pub cat: Vec<u32>,
    /// Category codes that map to TMDB "movie" media type.
    #[serde(default = "default_cat_movie")]
    pub cat_movie: Vec<u32>,
    /// TIDs excluded from display in the title viewer.
    #[serde(default = "default_excludes")]
    pub excludes: Vec<u32>,
}

impl Default for TitlesConfig {
    fn default() -> Self {
        Self {
            cat: default_cat(),
            cat_movie: default_cat_movie(),
            excludes: default_excludes(),
        }
    }
}

/// Channel selection configuration.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ChannelsConfig {
    /// Selected channel IDs (Syoboi `ChID`).
    #[serde(default)]
    pub selected: Vec<u32>,
}

/// TMDB settings.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TmdbConfig {
    /// Default language (e.g. "ja-JP"). Used when `--language` is not specified.
    #[serde(default)]
    pub language: Option<String>,
    /// API credentials.
    #[serde(default)]
    pub api: TmdbApiConfig,
}

/// TMDB API credentials.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TmdbApiConfig {
    /// API bearer token. Falls back when `TMDB_API_TOKEN` env var is not set.
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Normalize viewer settings.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct NormalizeConfig {
    /// Regex pattern history for the normalize viewer.
    #[serde(default)]
    pub regex_history: Vec<String>,
    /// Regex patterns for title normalization (combined with `|`).
    #[serde(default)]
    pub regex_titles: Vec<String>,
}

impl AppConfig {
    /// Loads config from a TOML file. Returns default if file does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))
    }

    /// Saves config to a TOML file, creating parent directories if needed.
    ///
    /// Unset optional values are written as commented-out lines so users can
    /// see all available options.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation or file write fails.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        let content = self.to_commented_toml();
        std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
    }

    /// Renders config as TOML with commented-out hints for unset options.
    #[allow(clippy::too_many_lines)]
    fn to_commented_toml(&self) -> String {
        let mut out = String::new();

        // [syoboi.channels]
        out.push_str("[syoboi.channels]\n");
        out.push_str("# Selected channel IDs (Syoboi ChID).\n");
        if self.syoboi.channels.selected.is_empty() {
            out.push_str("# selected = []\n");
        } else {
            let ids: Vec<String> = self
                .syoboi
                .channels
                .selected
                .iter()
                .map(ToString::to_string)
                .collect();
            let _ = writeln!(out, "selected = [{}]", ids.join(", "));
        }

        // [syoboi.titles]
        out.push_str("\n[syoboi.titles]\n");
        out.push_str("# Syoboi category codes to include.\n");
        out.push_str(
            "# 0: その他, 1: アニメ, 2: ラジオ, 3: テレビ, 4: 特撮,\n\
             # 5: アニメ関連, 6: メモ, 7: OVA, 8: 映画, 10: アニメ(終了/再放送)\n",
        );
        if self.syoboi.titles.cat.is_empty() {
            out.push_str("# cat = []\n");
        } else {
            let ids: Vec<String> = self
                .syoboi
                .titles
                .cat
                .iter()
                .map(ToString::to_string)
                .collect();
            let _ = writeln!(out, "cat = [{}]", ids.join(", "));
        }
        out.push_str("# Category codes that map to TMDB \"movie\" media type.\n");
        if self.syoboi.titles.cat_movie.is_empty() {
            out.push_str("# cat_movie = []\n");
        } else {
            let ids: Vec<String> = self
                .syoboi
                .titles
                .cat_movie
                .iter()
                .map(ToString::to_string)
                .collect();
            let _ = writeln!(out, "cat_movie = [{}]", ids.join(", "));
        }
        out.push_str("# TIDs excluded from display in the title viewer.\n");
        if self.syoboi.titles.excludes.is_empty() {
            out.push_str("# excludes = []\n");
        } else {
            let mut sorted: Vec<u32> = self.syoboi.titles.excludes.clone();
            sorted.sort_unstable();
            let ids: Vec<String> = sorted.iter().map(ToString::to_string).collect();
            let _ = writeln!(out, "excludes = [{}]", ids.join(", "));
        }

        // [tmdb]
        out.push_str("\n[tmdb]\n");
        out.push_str(
            "# Default language (e.g. \"ja-JP\"). Used when --language is not specified.\n",
        );
        let lang = self.tmdb.language.as_deref().unwrap_or("ja-JP");
        let _ = writeln!(out, "language = \"{lang}\"");

        // [tmdb.api]
        out.push_str("\n[tmdb.api]\n");
        out.push_str("# API bearer token. Falls back when TMDB_API_TOKEN env var is not set.\n");
        match &self.tmdb.api.api_key {
            Some(key) => {
                let _ = writeln!(out, "api_key = \"{key}\"");
            }
            None => out.push_str("# api_key = \"\"\n"),
        }

        // [normalize]
        out.push_str("\n[normalize]\n");
        out.push_str("# Regex pattern history for the normalize viewer.\n");
        if self.normalize.regex_history.is_empty() {
            out.push_str("# regex_history = []\n");
        } else {
            // Use TOML literal strings (single quotes) to avoid backslash escaping
            // issues with regex patterns like `\d+` and `\s+`.
            let patterns: Vec<String> = self
                .normalize
                .regex_history
                .iter()
                .map(|p| format!("'{p}'"))
                .collect();
            let _ = writeln!(out, "regex_history = [{}]", patterns.join(", "));
        }
        out.push_str("# Regex patterns for title normalization (combined with `|`).\n");
        if self.normalize.regex_titles.is_empty() {
            out.push_str("# regex_titles = []\n");
        } else {
            let patterns: Vec<String> = self
                .normalize
                .regex_titles
                .iter()
                .map(|p| format!("'{p}'"))
                .collect();
            let _ = writeln!(out, "regex_titles = [{}]", patterns.join(", "));
        }

        // [jlse.dirs] + [jlse.bins]
        out.push_str("\n# CM detection pipeline settings.\n");
        if let Some(jlse) = &self.jlse {
            out.push_str("[jlse.dirs]\n");
            let _ = writeln!(out, "jl = \"{}\"", jlse.dirs.jl.display());
            let _ = writeln!(out, "logo = \"{}\"", jlse.dirs.logo.display());
            let _ = writeln!(out, "result = \"{}\"", jlse.dirs.result.display());

            out.push_str("\n[jlse.bins]\n");
            let bins = &jlse.bins;
            let jl_bin_dir = jlse.dirs.bin_dir();
            Self::write_bin_field(
                &mut out,
                "logoframe",
                bins.logoframe.as_ref(),
                &jl_bin_dir,
                "logoframe",
            );
            Self::write_bin_field(
                &mut out,
                "chapter_exe",
                bins.chapter_exe.as_ref(),
                &jl_bin_dir,
                "chapter_exe",
            );
            Self::write_bin_field(
                &mut out,
                "tsdivider",
                bins.tsdivider.as_ref(),
                &jl_bin_dir,
                "tsdivider",
            );
            Self::write_bin_field(
                &mut out,
                "join_logo_scp",
                bins.join_logo_scp.as_ref(),
                &jl_bin_dir,
                "join_logo_scp",
            );
            Self::write_bin_field(
                &mut out,
                "ffprobe",
                bins.ffprobe.as_ref(),
                Path::new("/usr/local/bin"),
                "ffprobe",
            );
            Self::write_bin_field(
                &mut out,
                "ffmpeg",
                bins.ffmpeg.as_ref(),
                Path::new("/usr/local/bin"),
                "ffmpeg",
            );
        } else {
            out.push_str("# [jlse.dirs]\n");
            out.push_str("# jl = \"/path/to/JL\"\n");
            out.push_str("# logo = \"/path/to/logo\"\n");
            out.push_str("# result = \"/path/to/result\"\n");
            out.push_str("#\n");
            out.push_str("# [jlse.bins]\n");
            out.push_str("# logoframe = \"/path/to/bin/logoframe\"\n");
            out.push_str("# chapter_exe = \"/path/to/bin/chapter_exe\"\n");
            out.push_str("# tsdivider = \"/path/to/bin/tsdivider\"\n");
            out.push_str("# join_logo_scp = \"/path/to/bin/join_logo_scp\"\n");
            out.push_str("# ffprobe = \"/usr/local/bin/ffprobe\"\n");
            out.push_str("# ffmpeg = \"/usr/local/bin/ffmpeg\"\n");
        }

        out
    }

    /// Write a single binary field as active or commented-out line.
    fn write_bin_field(
        out: &mut String,
        key: &str,
        value: Option<&PathBuf>,
        default_dir: &Path,
        default_name: &str,
    ) {
        match value {
            Some(p) => {
                let _ = writeln!(out, "{key} = \"{}\"", p.display());
            }
            None => {
                let _ = writeln!(
                    out,
                    "# {key} = \"{}\"",
                    default_dir.join(default_name).display()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn test_default_config() {
        // Arrange & Act
        let config = AppConfig::default();

        // Assert
        assert!(config.syoboi.channels.selected.is_empty());
        assert!(config.tmdb.language.is_none());
        assert!(config.tmdb.api.api_key.is_none());
        assert!(config.normalize.regex_history.is_empty());
        assert!(config.normalize.regex_titles.is_empty());
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        // Arrange
        let config = AppConfig {
            syoboi: SyoboiConfig {
                channels: ChannelsConfig {
                    selected: vec![1, 2, 3, 7, 19],
                },
                ..SyoboiConfig::default()
            },
            tmdb: TmdbConfig {
                language: Some(String::from("ja-JP")),
                api: TmdbApiConfig {
                    api_key: Some(String::from("test-key")),
                },
            },
            normalize: NormalizeConfig {
                regex_history: vec![
                    String::from(r"第(?P<SeasonNum>\d+)期"),
                    String::from(r"Season\s+(?P<SeasonNum>\d+)"),
                ],
                regex_titles: vec![String::from(r"第\d+期$"), String::from(r"\s*Season\s*\d+")],
            },
            jlse: None,
        };

        // Act
        let toml_str = config.to_commented_toml();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();

        // Assert
        assert_eq!(parsed, config);
    }

    #[test]
    fn test_commented_toml_default() {
        // Arrange
        let config = AppConfig::default();

        // Act
        let output = config.to_commented_toml();

        // Assert — unset options are commented out, language defaults to "ja-JP"
        assert!(output.contains("# selected = []"));
        assert!(output.contains("cat = [1, 7, 8, 10]"));
        assert!(output.contains("cat_movie = [8]"));
        assert!(output.contains("excludes = [5, 44, 46,"));
        assert!(output.contains("language = \"ja-JP\""));
        assert!(!output.contains("# language"));
        assert!(output.contains("# api_key = \"\""));
        assert!(output.contains("# regex_history = []"));
        assert!(output.contains("# regex_titles = []"));
        // Parsed config gets "ja-JP" from the active line
        let parsed: AppConfig = toml::from_str(&output).unwrap();
        assert_eq!(parsed.tmdb.language, Some(String::from("ja-JP")));
        assert_eq!(parsed.syoboi.titles.cat, vec![1, 7, 8, 10]);
        assert_eq!(parsed.syoboi.titles.cat_movie, vec![8]);
    }

    #[test]
    fn test_commented_toml_with_values() {
        // Arrange
        let config = AppConfig {
            syoboi: SyoboiConfig {
                channels: ChannelsConfig {
                    selected: vec![1, 7],
                },
                ..SyoboiConfig::default()
            },
            tmdb: TmdbConfig {
                language: Some(String::from("en-US")),
                api: TmdbApiConfig {
                    api_key: Some(String::from("my-token")),
                },
            },
            normalize: NormalizeConfig {
                regex_history: vec![String::from(r"第(?P<SeasonNum>\d+)期")],
                regex_titles: vec![String::from(r"第\d+期$"), String::from(r"\s*Season\s*\d+")],
            },
            jlse: None,
        };

        // Act
        let output = config.to_commented_toml();

        // Assert — active values are not commented
        assert!(output.contains("selected = [1, 7]"));
        assert!(!output.contains("# selected"));
        assert!(output.contains("cat = [1, 7, 8, 10]"));
        assert!(output.contains("cat_movie = [8]"));
        assert!(output.contains("language = \"en-US\""));
        assert!(output.contains("api_key = \"my-token\""));
        assert!(output.contains(r"regex_history = ['第(?P<SeasonNum>\d+)期']"));
        assert!(output.contains(r"regex_titles = ['第\d+期$', '\s*Season\s*\d+']"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_nonexistent_returns_default() {
        // Arrange
        let path = Path::new("/tmp/dtvmgr_test_nonexistent_config.toml");

        // Act
        let config = AppConfig::load(path).unwrap();

        // Assert
        assert_eq!(config, AppConfig::default());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_save_and_load() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dtvmgr.toml");
        let config = AppConfig {
            syoboi: SyoboiConfig {
                channels: ChannelsConfig {
                    selected: vec![1, 3, 7],
                },
                ..SyoboiConfig::default()
            },
            ..AppConfig::default()
        };

        // Act
        config.save(&path).unwrap();
        let loaded = AppConfig::load(&path).unwrap();

        // Assert — channels preserved, language gets "ja-JP" from default output
        assert_eq!(loaded.syoboi.channels.selected, vec![1, 3, 7]);
        assert_eq!(loaded.tmdb.language, Some(String::from("ja-JP")));
        assert!(loaded.tmdb.api.api_key.is_none());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_partial_config() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();

        // Act
        let config = AppConfig::load(&path).unwrap();

        // Assert
        assert_eq!(config, AppConfig::default());
    }

    #[test]
    fn test_jlse_roundtrip() {
        use dtvmgr_jlse::types::{JlseBins, JlseDirs};

        // Arrange
        let config = AppConfig {
            tmdb: TmdbConfig {
                language: Some(String::from("ja-JP")),
                ..TmdbConfig::default()
            },
            jlse: Some(JlseConfig {
                dirs: JlseDirs {
                    jl: PathBuf::from("/opt/module/JL"),
                    logo: PathBuf::from("/opt/module/logo"),
                    result: PathBuf::from("/tmp/result"),
                },
                bins: JlseBins::default(),
            }),
            ..AppConfig::default()
        };

        // Act
        let toml_str = config.to_commented_toml();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();

        // Assert
        assert_eq!(parsed, config);
    }

    #[test]
    fn test_jlse_with_bins_override_roundtrip() {
        use dtvmgr_jlse::types::{JlseBins, JlseDirs};

        // Arrange
        let config = AppConfig {
            tmdb: TmdbConfig {
                language: Some(String::from("ja-JP")),
                ..TmdbConfig::default()
            },
            jlse: Some(JlseConfig {
                dirs: JlseDirs {
                    jl: PathBuf::from("/opt/module/JL"),
                    logo: PathBuf::from("/opt/module/logo"),
                    result: PathBuf::from("/tmp/result"),
                },
                bins: JlseBins {
                    ffmpeg: Some(PathBuf::from("/usr/bin/ffmpeg")),
                    ..JlseBins::default()
                },
            }),
            ..AppConfig::default()
        };

        // Act
        let toml_str = config.to_commented_toml();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();

        // Assert
        assert_eq!(parsed, config);
    }
}
