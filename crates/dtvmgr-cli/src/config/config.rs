//! `AppConfig` struct and TOML read/write.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use dtvmgr_jlse::types::{DurationCheckRule, JlseBins, JlseConfig, JlseDirs, JlseEncode};
use dtvmgr_jlse::validate::DEFAULT_RULES;
use serde::{Deserialize, Serialize};

/// Top-level application configuration.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct AppConfig {
    /// Syoboi Calendar settings.
    #[serde(default)]
    pub syoboi: SyoboiConfig,
    /// TMDB settings.
    #[serde(default)]
    pub tmdb: TmdbConfig,
    /// `EPGStation` settings.
    #[serde(default)]
    pub epgstation: EpgStationConfig,
    /// Normalize viewer settings.
    #[serde(default)]
    pub normalize: NormalizeConfig,
    /// CM detection pipeline settings.
    #[serde(default)]
    pub jlse: Option<JlseConfig>,
}

/// `EPGStation` settings.
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EpgStationConfig {
    /// Base URL (e.g. `http://localhost:8888`).
    #[serde(default)]
    pub base_url: Option<String>,
    /// Default sub-directory for encoded files.
    #[serde(default)]
    pub default_directory: Option<String>,
    /// Default encode preset name (e.g. "H.264").
    #[serde(default)]
    pub default_preset: Option<String>,
    /// Storage directory names hidden in the TUI widget.
    #[serde(default)]
    pub hidden_storage_dirs: Vec<String>,
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
    /// API bearer token. Falls back when `TMDB_API_TOKEN` env var is not set.
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Default regex pattern history.
fn default_regex_history() -> Vec<String> {
    vec![r"\(.*\)$".to_owned(), r"\s?\(.*\)$".to_owned()]
}

/// Default regex patterns for title normalization.
fn default_regex_titles() -> Vec<String> {
    vec![
        r"\s*\(第\d+(?:期|クール|シリーズ)\)".to_owned(),
        r"\s*第\d+(?:期|クール|シリーズ)".to_owned(),
        r"(?i:\s*\d+(?:st|nd|rd|th)\s+season)".to_owned(),
        r"(?i:\s*season\s*\d+(?:\s+part\.?\s*\d+)?)".to_owned(),
        r"(?i:\s*(?:the\s+)?final\s+season)".to_owned(),
        r"\s*\(シーズン\d+\)".to_owned(),
        r"\s*シーズン\s*\d+".to_owned(),
        r"\s*\(\d\d?\)".to_owned(),
        r"\s*~(.*)~$".to_owned(),
        r"^映画\s?".to_owned(),
        r"\(TVシリーズ\)".to_owned(),
    ]
}

/// Normalize viewer settings.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NormalizeConfig {
    /// Regex pattern history for the normalize viewer.
    #[serde(default = "default_regex_history")]
    pub regex_history: Vec<String>,
    /// Regex patterns for title normalization (combined with `|`).
    #[serde(default = "default_regex_titles")]
    pub regex_titles: Vec<String>,
}

impl Default for NormalizeConfig {
    fn default() -> Self {
        Self {
            regex_history: default_regex_history(),
            regex_titles: default_regex_titles(),
        }
    }
}

impl AppConfig {
    /// Loads config from a TOML file.
    ///
    /// If the file does not exist, returns `Self::default()` and attempts to
    /// write a commented template to `path` so users can discover all options.
    /// Template write failure is logged but does not cause an error.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                tracing::info!(path = %path.display(), "loaded config");
                toml::from_str(&content)
                    .with_context(|| format!("failed to parse {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(path = %path.display(), "config not found, using defaults");
                let config = Self::default();
                let content = config.to_commented_toml();
                // Best-effort write so users can discover all options.
                if let Err(save_err) = Self::write_toml(path, &content) {
                    tracing::warn!(
                        path = %path.display(),
                        error = %save_err,
                        "could not write default config template"
                    );
                }
                // Parse the generated template directly (avoids re-reading
                // from disk) so active sections like jlse are included.
                toml::from_str(&content)
                    .with_context(|| "failed to parse default config template".to_owned())
            }
            Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
        }
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
        let content = self.to_commented_toml();
        Self::write_toml(path, &content)
    }

    /// Write TOML content to `path`, creating parent directories as needed.
    pub(crate) fn write_toml(path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
    }

    /// Renders config as TOML with commented-out hints for unset options.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn to_commented_toml(&self) -> String {
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
        let mut sorted_excludes = self.syoboi.titles.excludes.clone();
        sorted_excludes.sort_unstable();
        let mut entries: Vec<(&str, String)> = vec![
            (
                "cat",
                Self::format_list(
                    "cat",
                    &self.syoboi.titles.cat,
                    ToString::to_string,
                    Some(
                        "# Syoboi category codes to include.\n\
                         # 0: その他, 1: アニメ, 2: ラジオ, 3: テレビ, 4: 特撮,\n\
                         # 5: アニメ関連, 6: メモ, 7: OVA, 8: 映画, 10: アニメ(終了/再放送)\n",
                    ),
                ),
            ),
            (
                "cat_movie",
                Self::format_list(
                    "cat_movie",
                    &self.syoboi.titles.cat_movie,
                    ToString::to_string,
                    Some("# Category codes that map to TMDB \"movie\" media type.\n"),
                ),
            ),
            (
                "excludes",
                Self::format_list(
                    "excludes",
                    &sorted_excludes,
                    ToString::to_string,
                    Some("# TIDs excluded from display in the title viewer.\n"),
                ),
            ),
        ];
        Self::write_sorted_entries(&mut out, &mut entries);

        // [tmdb]
        out.push_str("\n[tmdb]\n");
        out.push_str(
            "# Default language (e.g. \"ja-JP\"). Used when --language is not specified.\n",
        );
        let lang = self.tmdb.language.as_deref().unwrap_or("ja-JP");
        let _ = writeln!(out, "language = \"{lang}\"");
        out.push_str("# API bearer token. Falls back when TMDB_API_TOKEN env var is not set.\n");
        out.push_str(&Self::format_optional_str(
            "api_key",
            self.tmdb.api_key.as_deref(),
            "",
        ));

        // [epgstation]
        out.push_str("\n[epgstation]\n");
        out.push_str("# Base URL (e.g. \"http://localhost:8888\").\n");
        out.push_str(&Self::format_optional_str(
            "base_url",
            self.epgstation.base_url.as_deref(),
            "http://localhost:8888",
        ));
        out.push_str("# Default sub-directory for encoded files.\n");
        out.push_str(&Self::format_optional_str(
            "default_directory",
            self.epgstation.default_directory.as_deref(),
            "",
        ));
        out.push_str("# Default encode preset name (e.g. \"H.264\").\n");
        out.push_str(&Self::format_optional_str(
            "default_preset",
            self.epgstation.default_preset.as_deref(),
            "",
        ));
        out.push_str(&Self::format_list(
            "hidden_storage_dirs",
            &self.epgstation.hidden_storage_dirs,
            |s| format!("\"{s}\""),
            Some("# Storage directory names hidden in the TUI widget.\n"),
        ));

        // [normalize]
        out.push_str("\n[normalize]\n");
        let mut entries: Vec<(&str, String)> = vec![
            (
                "regex_history",
                Self::format_list(
                    "regex_history",
                    &self.normalize.regex_history,
                    |p| format!("'{p}'"),
                    Some("# Regex pattern history for the normalize viewer.\n"),
                ),
            ),
            (
                "regex_titles",
                Self::format_list(
                    "regex_titles",
                    &self.normalize.regex_titles,
                    |p| format!("'{p}'"),
                    Some("# Regex patterns for title normalization (combined with `|`).\n"),
                ),
            ),
        ];
        Self::write_sorted_entries(&mut out, &mut entries);

        // [jlse] — all sections always active with defaults
        out.push_str("\n# CM detection pipeline settings.\n");
        let default_jlse = JlseConfig {
            dirs: JlseDirs::default(),
            bins: JlseBins::default(),
            encode: Some(JlseEncode::default()),
        };
        let jlse = self.jlse.as_ref().unwrap_or(&default_jlse);

        out.push_str("[jlse.dirs]\n");
        let mut entries: Vec<(&str, String)> = vec![
            ("jl", Self::format_path("jl", &jlse.dirs.jl)),
            ("logo", Self::format_path("logo", &jlse.dirs.logo)),
            ("result", Self::format_path("result", &jlse.dirs.result)),
        ];
        Self::write_sorted_entries(&mut out, &mut entries);

        Self::write_bins_active(&mut out, &jlse.bins, &default_jlse.bins);

        let default_enc = JlseEncode::default();
        let enc = jlse.encode.as_ref().unwrap_or(&default_enc);
        out.push_str(&Self::write_encode_active(enc));

        out
    }

    /// Render encode config as active (uncommented) TOML lines.
    fn write_encode_active(enc: &JlseEncode) -> String {
        let mut out = String::new();
        out.push_str("\n[jlse.encode]\n");
        out.push_str(&Self::format_optional_str(
            "format",
            enc.format.as_deref(),
            "mkv",
        ));

        if let Some(ref input) = enc.input {
            Self::write_encode_input(&mut out, input);
        }
        if let Some(ref video) = enc.video {
            Self::write_encode_video(&mut out, video);
        }
        if let Some(ref audio) = enc.audio {
            Self::write_encode_audio(&mut out, audio);
        }

        Self::write_duration_check(&mut out, enc.duration_check.as_deref());

        Self::write_quality_search(&mut out, enc.quality_search.as_ref());

        out
    }

    /// Write `[jlse.encode.input]` section with sorted keys.
    fn write_encode_input(out: &mut String, input: &dtvmgr_jlse::types::EncodeInput) {
        out.push_str("\n[jlse.encode.input]\n");
        let mut entries: Vec<(&str, String)> = vec![
            (
                "analyzeduration",
                Self::format_optional_str(
                    "analyzeduration",
                    input.analyzeduration.as_deref(),
                    "30M",
                ),
            ),
            (
                "decoder",
                Self::format_optional_str("decoder", input.decoder.as_deref(), "mpeg2_qsv"),
            ),
            (
                "filter_hw_device",
                Self::format_optional_str(
                    "filter_hw_device",
                    input.filter_hw_device.as_deref(),
                    "hw",
                ),
            ),
            (
                "flags",
                Self::format_optional_str(
                    "flags",
                    input.flags.as_deref(),
                    "+discardcorrupt+genpts",
                ),
            ),
            (
                "hwaccel",
                Self::format_optional_str("hwaccel", input.hwaccel.as_deref(), "qsv"),
            ),
            (
                "hwaccel_output_format",
                Self::format_optional_str(
                    "hwaccel_output_format",
                    input.hwaccel_output_format.as_deref(),
                    "qsv",
                ),
            ),
            (
                "init_hw_device",
                Self::format_optional_str(
                    "init_hw_device",
                    input.init_hw_device.as_deref(),
                    "qsv=hw",
                ),
            ),
            (
                "probesize",
                Self::format_optional_str("probesize", input.probesize.as_deref(), "100M"),
            ),
        ];
        Self::write_sorted_entries(out, &mut entries);
    }

    /// Write `[jlse.encode.video]` section with sorted keys.
    fn write_encode_video(out: &mut String, video: &dtvmgr_jlse::types::EncodeVideo) {
        out.push_str("\n[jlse.encode.video]\n");
        let mut entries: Vec<(&str, String)> = vec![
            (
                "aspect",
                Self::format_optional_str("aspect", video.aspect.as_deref(), "16:9"),
            ),
            (
                "codec",
                Self::format_optional_str("codec", video.codec.as_deref(), "libx264"),
            ),
            (
                "filter",
                Self::format_optional_str(
                    "filter",
                    video.filter.as_deref(),
                    "yadif=mode=send_frame:parity=auto:deint=all,scale=w=1280:h=720",
                ),
            ),
            (
                "pix_fmt",
                Self::format_optional_str("pix_fmt", video.pix_fmt.as_deref(), "yuv420p"),
            ),
            (
                "preset",
                Self::format_optional_str("preset", video.preset.as_deref(), "medium"),
            ),
            (
                "profile",
                Self::format_optional_str("profile", video.profile.as_deref(), "main"),
            ),
        ];
        Self::write_sorted_entries(out, &mut entries);
        out.push_str(&Self::format_list(
            "extra",
            &video.extra,
            |v| format!("\"{v}\""),
            None,
        ));
    }

    /// Write `[jlse.encode.audio]` section with sorted keys.
    fn write_encode_audio(out: &mut String, audio: &dtvmgr_jlse::types::EncodeAudio) {
        out.push_str("\n[jlse.encode.audio]\n");
        let mut entries: Vec<(&str, String)> = vec![
            (
                "bitrate",
                Self::format_optional_str("bitrate", audio.bitrate.as_deref(), "256k"),
            ),
            (
                "channels",
                Self::format_optional_u32("channels", audio.channels, 2),
            ),
            (
                "codec",
                Self::format_optional_str("codec", audio.codec.as_deref(), "aac"),
            ),
            (
                "sample_rate",
                Self::format_optional_u32("sample_rate", audio.sample_rate, 48000),
            ),
        ];
        Self::write_sorted_entries(out, &mut entries);
        out.push_str(&Self::format_list(
            "extra",
            &audio.extra,
            |v| format!("\"{v}\""),
            None,
        ));
    }

    /// Write `[[jlse.encode.duration_check]]` section.
    ///
    /// When the user has configured custom rules, they are written as active
    /// TOML array-of-tables entries. Otherwise, defaults are written as
    /// comments so users can see the available fields and their values.
    fn write_duration_check(out: &mut String, rules: Option<&[DurationCheckRule]>) {
        out.push_str(
            "\n# Pre-encode duration validation rules.\n\
             # Each entry defines the minimum acceptable content ratio\n\
             # for a program length range (in minutes).\n",
        );
        match rules {
            Some(entries) if !entries.is_empty() => {
                for r in entries {
                    out.push_str("\n[[jlse.encode.duration_check]]\n");
                    let mut e: Vec<(&str, String)> = vec![
                        ("max_min", format!("max_min = {}\n", r.max_min)),
                        ("min_min", format!("min_min = {}\n", r.min_min)),
                        ("min_percent", format!("min_percent = {}\n", r.min_percent)),
                    ];
                    Self::write_sorted_entries(out, &mut e);
                }
            }
            _ => {
                for r in DEFAULT_RULES {
                    out.push_str("# [[jlse.encode.duration_check]]\n");
                    let mut e: Vec<(&str, String)> = vec![
                        ("max_min", format!("# max_min = {}\n", r.max_min)),
                        ("min_min", format!("# min_min = {}\n", r.min_min)),
                        (
                            "min_percent",
                            format!("# min_percent = {}\n", r.min_percent),
                        ),
                    ];
                    Self::write_sorted_entries(out, &mut e);
                }
            }
        }
    }

    /// Write `[jlse.encode.quality_search]` section.
    ///
    /// When the user has configured quality search, active values are written.
    /// Otherwise, defaults are written as comments so users can see the
    /// available fields.
    fn write_quality_search(
        out: &mut String,
        qs: Option<&dtvmgr_jlse::types::QualitySearchConfig>,
    ) {
        out.push_str(
            "\n# VMAF-based quality parameter search.\n\
             # When enabled, automatically determines the optimal CRF/ICQ\n\
             # by sampling the input and measuring VMAF scores.\n",
        );

        match qs {
            Some(q) if q.enabled => {
                out.push_str("\n[jlse.encode.quality_search]\n");
                let mut entries: Vec<(&str, String)> = vec![
                    ("enabled", String::from("enabled = true\n")),
                    (
                        "max_encoded_percent",
                        Self::format_optional_f32(
                            "max_encoded_percent",
                            q.max_encoded_percent,
                            80.0,
                        ),
                    ),
                    (
                        "max_samples",
                        Self::format_optional_u32("max_samples", q.max_samples, 15),
                    ),
                    (
                        "min_samples",
                        Self::format_optional_u32("min_samples", q.min_samples, 5),
                    ),
                    (
                        "sample_duration_secs",
                        Self::format_optional_f64(
                            "sample_duration_secs",
                            q.sample_duration_secs,
                            3.0,
                        ),
                    ),
                    (
                        "sample_every_secs",
                        Self::format_optional_f64("sample_every_secs", q.sample_every_secs, 720.0),
                    ),
                    (
                        "skip_secs",
                        Self::format_optional_f64("skip_secs", q.skip_secs, 120.0),
                    ),
                    (
                        "vmaf_subsample",
                        Self::format_optional_u32("vmaf_subsample", q.vmaf_subsample, 5),
                    ),
                    (
                        "target_vmaf",
                        Self::format_optional_f32("target_vmaf", q.target_vmaf, 93.0),
                    ),
                    (
                        "thorough",
                        q.thorough.map_or_else(
                            || String::from("# thorough = true\n"),
                            |v| format!("thorough = {v}\n"),
                        ),
                    ),
                ];
                Self::write_sorted_entries(out, &mut entries);
            }
            _ => {
                out.push_str("# [jlse.encode.quality_search]\n");
                let mut entries: Vec<(&str, String)> = vec![
                    ("enabled", String::from("# enabled = true\n")),
                    (
                        "max_encoded_percent",
                        String::from("# max_encoded_percent = 80\n"),
                    ),
                    ("max_samples", String::from("# max_samples = 15\n")),
                    ("min_samples", String::from("# min_samples = 5\n")),
                    (
                        "sample_duration_secs",
                        String::from("# sample_duration_secs = 3\n"),
                    ),
                    (
                        "sample_every_secs",
                        String::from("# sample_every_secs = 720\n"),
                    ),
                    ("skip_secs", String::from("# skip_secs = 120\n")),
                    ("target_vmaf", String::from("# target_vmaf = 93.0\n")),
                    ("thorough", String::from("# thorough = true\n")),
                    ("vmaf_subsample", String::from("# vmaf_subsample = 5\n")),
                ];
                Self::write_sorted_entries(out, &mut entries);
            }
        }
    }

    /// Format an always-active path field.
    fn format_path(key: &str, value: &Path) -> String {
        format!("{key} = \"{}\"\n", value.display())
    }

    /// Format an optional path field (active or commented).
    fn format_optional_path(key: &str, value: Option<&Path>, hint: Option<&Path>) -> String {
        value.map_or_else(
            || hint.map_or_else(String::new, |h| format!("# {key} = \"{}\"\n", h.display())),
            |p| format!("{key} = \"{}\"\n", p.display()),
        )
    }

    /// Format a list field with optional preceding comment.
    ///
    /// Each element is converted via `formatter`; the result is joined with
    /// `, ` inside `[…]`.  An empty slice renders a commented-out line.
    fn format_list<T>(
        key: &str,
        values: &[T],
        formatter: impl Fn(&T) -> String,
        comment: Option<&str>,
    ) -> String {
        let mut s = String::new();
        if let Some(c) = comment {
            s.push_str(c);
        }
        if values.is_empty() {
            let _ = writeln!(s, "# {key} = []");
        } else {
            let items: Vec<String> = values.iter().map(formatter).collect();
            let _ = writeln!(s, "{key} = [{}]", items.join(", "));
        }
        s
    }

    /// Format an optional string field as a TOML line (active or commented).
    fn format_optional_str(key: &str, value: Option<&str>, hint: &str) -> String {
        value.map_or_else(
            || format!("# {key} = \"{hint}\"\n"),
            |v| format!("{key} = \"{v}\"\n"),
        )
    }

    /// Format an optional u32 field as a TOML line (active or commented).
    fn format_optional_u32(key: &str, value: Option<u32>, hint: u32) -> String {
        value.map_or_else(
            || format!("# {key} = {hint}\n"),
            |v| format!("{key} = {v}\n"),
        )
    }

    /// Format an optional f32 field as a TOML line (active or commented).
    fn format_optional_f32(key: &str, value: Option<f32>, hint: f32) -> String {
        value.map_or_else(
            || format!("# {key} = {hint}\n"),
            |v| format!("{key} = {v}\n"),
        )
    }

    /// Format an optional f64 field as a TOML line (active or commented).
    fn format_optional_f64(key: &str, value: Option<f64>, hint: f64) -> String {
        value.map_or_else(
            || format!("# {key} = {hint}\n"),
            |v| format!("{key} = {v}\n"),
        )
    }

    /// Write entries sorted alphabetically by key.
    fn write_sorted_entries(out: &mut String, entries: &mut [(&str, String)]) {
        entries.sort_unstable_by_key(|(key, _)| *key);
        for (_, line) in entries {
            out.push_str(line);
        }
    }

    /// Write `[jlse.bins]` section with all binary paths.
    fn write_bins_active(out: &mut String, bins: &JlseBins, defaults: &JlseBins) {
        out.push_str("\n[jlse.bins]\n");
        let mut entries: Vec<(&str, String)> = vec![
            (
                "chapter_exe",
                Self::format_optional_path(
                    "chapter_exe",
                    bins.chapter_exe.as_deref(),
                    defaults.chapter_exe.as_deref(),
                ),
            ),
            (
                "ffmpeg",
                Self::format_optional_path(
                    "ffmpeg",
                    bins.ffmpeg.as_deref(),
                    defaults.ffmpeg.as_deref(),
                ),
            ),
            (
                "ffprobe",
                Self::format_optional_path(
                    "ffprobe",
                    bins.ffprobe.as_deref(),
                    defaults.ffprobe.as_deref(),
                ),
            ),
            (
                "join_logo_scp",
                Self::format_optional_path(
                    "join_logo_scp",
                    bins.join_logo_scp.as_deref(),
                    defaults.join_logo_scp.as_deref(),
                ),
            ),
            (
                "logoframe",
                Self::format_optional_path(
                    "logoframe",
                    bins.logoframe.as_deref(),
                    defaults.logoframe.as_deref(),
                ),
            ),
            (
                "tstables",
                Self::format_optional_path(
                    "tstables",
                    bins.tstables.as_deref(),
                    defaults.tstables.as_deref(),
                ),
            ),
        ];
        Self::write_sorted_entries(out, &mut entries);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_default_config() {
        // Arrange & Act
        let config = AppConfig::default();

        // Assert
        assert!(config.syoboi.channels.selected.is_empty());
        assert!(config.tmdb.language.is_none());
        assert!(config.tmdb.api_key.is_none());
        assert!(config.epgstation.base_url.is_none());
        assert!(config.epgstation.default_directory.is_none());
        assert!(config.epgstation.default_preset.is_none());
        assert_eq!(config.normalize.regex_history, default_regex_history());
        assert_eq!(config.normalize.regex_titles, default_regex_titles());
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
                api_key: Some(String::from("test-key")),
            },
            epgstation: EpgStationConfig::default(),
            normalize: NormalizeConfig {
                regex_history: vec![
                    String::from(r"第(?P<SeasonNum>\d+)期"),
                    String::from(r"Season\s+(?P<SeasonNum>\d+)"),
                ],
                regex_titles: vec![String::from(r"第\d+期$"), String::from(r"\s*Season\s*\d+")],
            },
            jlse: None,
        };

        // Act — encode is always active, so jlse becomes Some after roundtrip
        let toml_str = config.to_commented_toml();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();

        // Assert — non-jlse fields match exactly
        assert_eq!(parsed.syoboi, config.syoboi);
        assert_eq!(parsed.tmdb, config.tmdb);
        assert_eq!(parsed.normalize, config.normalize);
        // jlse gains defaults after roundtrip
        let jlse = parsed.jlse.unwrap();
        assert_eq!(jlse.dirs, JlseDirs::default());
        assert_eq!(jlse.bins, JlseBins::default());
        assert_eq!(jlse.encode, Some(JlseEncode::default()));
    }

    #[test]
    fn test_serialize_deserialize_roundtrip_with_hwaccel() {
        use dtvmgr_jlse::types::{EncodeInput, JlseBins, JlseDirs};

        // Arrange
        let config = AppConfig {
            tmdb: TmdbConfig {
                language: Some(String::from("ja-JP")),
                ..TmdbConfig::default()
            },
            jlse: Some(JlseConfig {
                dirs: JlseDirs {
                    jl: PathBuf::from("/opt/JL"),
                    logo: PathBuf::from("/opt/logo"),
                    result: PathBuf::from("/tmp/result"),
                },
                bins: JlseBins::default(),
                encode: Some(JlseEncode {
                    format: Some(String::from("mkv")),
                    input: Some(EncodeInput {
                        flags: Some(String::from("+discardcorrupt+genpts")),
                        analyzeduration: Some(String::from("30M")),
                        probesize: Some(String::from("100M")),
                        init_hw_device: None,
                        filter_hw_device: None,
                        hwaccel: Some(String::from("qsv")),
                        hwaccel_output_format: Some(String::from("qsv")),
                        decoder: Some(String::from("mpeg2_qsv")),
                    }),
                    video: None,
                    audio: None,
                    duration_check: None,
                    quality_search: None,
                }),
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
        // EPGStation section defaults are commented out
        assert!(output.contains("[epgstation]"));
        assert!(output.contains("# base_url = \"http://localhost:8888\""));
        assert!(output.contains(r"regex_history = ['\(.*\)$'"));
        assert!(output.contains(r"regex_titles = ['\s*\(第\d+(?:期|クール|シリーズ)\)'"));
        // hw device init fields are commented out (None in default)
        assert!(output.contains("# init_hw_device = \"qsv=hw\""));
        assert!(output.contains("# filter_hw_device = \"hw\""));
        // hwaccel fields are commented out (None in default)
        assert!(output.contains("# hwaccel = \"qsv\""));
        assert!(output.contains("# hwaccel_output_format = \"qsv\""));
        assert!(output.contains("# decoder = \"mpeg2_qsv\""));
        // Encode section headers are active (not commented)
        assert!(output.contains("[jlse.encode]\n"));
        assert!(output.contains("[jlse.encode.input]\n"));
        assert!(output.contains("[jlse.encode.video]\n"));
        assert!(output.contains("[jlse.encode.audio]\n"));
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
                api_key: Some(String::from("my-token")),
            },
            epgstation: EpgStationConfig::default(),
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
    fn test_load_nonexistent_creates_template() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new_config.toml");
        assert!(!path.exists());

        // Act
        let config = AppConfig::load(&path).unwrap();

        // Assert — template file created and re-read includes active jlse
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[jlse.dirs]"));
        assert!(content.contains("[jlse.bins]"));
        assert!(content.contains("[jlse.encode]"));
        // Re-read parses active sections, so jlse is Some
        assert!(config.jlse.is_some());
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
        assert!(loaded.tmdb.api_key.is_none());
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
                encode: None,
            }),
            ..AppConfig::default()
        };

        // Act — encode: None is written as active defaults
        let toml_str = config.to_commented_toml();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();

        // Assert — encode becomes Some(default) after roundtrip
        assert_eq!(
            parsed.jlse.as_ref().unwrap().dirs,
            config.jlse.as_ref().unwrap().dirs
        );
        assert_eq!(
            parsed.jlse.as_ref().unwrap().bins,
            config.jlse.as_ref().unwrap().bins
        );
        assert_eq!(
            parsed.jlse.as_ref().unwrap().encode,
            Some(JlseEncode::default())
        );
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
                encode: None,
            }),
            ..AppConfig::default()
        };

        // Act
        let toml_str = config.to_commented_toml();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();

        // Assert — bins override preserved, encode becomes Some(default)
        assert_eq!(
            parsed.jlse.as_ref().unwrap().dirs,
            config.jlse.as_ref().unwrap().dirs
        );
        assert_eq!(
            parsed.jlse.as_ref().unwrap().bins,
            config.jlse.as_ref().unwrap().bins
        );
        assert_eq!(
            parsed.jlse.as_ref().unwrap().encode,
            Some(JlseEncode::default())
        );
    }

    // ── format helpers ───────────────────────────────────────────

    #[test]
    fn test_format_optional_path_with_value() {
        let result = AppConfig::format_optional_path(
            "key",
            Some(Path::new("/tmp/file")),
            Some(Path::new("/hint")),
        );
        assert_eq!(result, "key = \"/tmp/file\"\n");
    }

    #[test]
    fn test_format_optional_path_hint_only() {
        let result = AppConfig::format_optional_path("key", None, Some(Path::new("/hint/path")));
        assert_eq!(result, "# key = \"/hint/path\"\n");
    }

    #[test]
    fn test_format_optional_path_none_none() {
        let result = AppConfig::format_optional_path("key", None, None);
        assert_eq!(result, "");
    }

    #[test]
    fn test_format_optional_u32_with_value() {
        let result = AppConfig::format_optional_u32("threads", Some(4), 8);
        assert_eq!(result, "threads = 4\n");
    }

    #[test]
    fn test_format_optional_u32_hint_only() {
        let result = AppConfig::format_optional_u32("threads", None, 8);
        assert_eq!(result, "# threads = 8\n");
    }

    #[test]
    fn test_format_optional_str_with_value() {
        let result = AppConfig::format_optional_str("name", Some("test"), "default");
        assert_eq!(result, "name = \"test\"\n");
    }

    #[test]
    fn test_format_optional_str_hint_only() {
        let result = AppConfig::format_optional_str("name", None, "default");
        assert_eq!(result, "# name = \"default\"\n");
    }

    #[test]
    fn test_format_list_with_values() {
        let result = AppConfig::format_list("items", &[1, 2, 3], ToString::to_string, None);
        assert_eq!(result, "items = [1, 2, 3]\n");
    }

    #[test]
    fn test_format_list_empty() {
        let result = AppConfig::format_list::<i32>("items", &[], ToString::to_string, None);
        assert_eq!(result, "# items = []\n");
    }

    #[test]
    fn test_format_list_with_comment() {
        let result =
            AppConfig::format_list("items", &[1], ToString::to_string, Some("# comment\n"));
        assert!(result.starts_with("# comment\n"));
        assert!(result.contains("items = [1]"));
    }

    // ── format_optional_f32 / format_optional_f64 ─────────────────

    #[test]
    fn test_format_optional_f32_with_value() {
        let result = AppConfig::format_optional_f32("score", Some(95.5), 93.0);
        assert_eq!(result, "score = 95.5\n");
    }

    #[test]
    fn test_format_optional_f32_hint_only() {
        let result = AppConfig::format_optional_f32("score", None, 93.0);
        assert_eq!(result, "# score = 93\n");
    }

    #[test]
    fn test_format_optional_f64_with_value() {
        let result = AppConfig::format_optional_f64("duration", Some(3.5), 3.0);
        assert_eq!(result, "duration = 3.5\n");
    }

    #[test]
    fn test_format_optional_f64_hint_only() {
        let result = AppConfig::format_optional_f64("duration", None, 3.0);
        assert_eq!(result, "# duration = 3\n");
    }

    // ── write_quality_search ──────────────────────────────────────

    #[test]
    fn test_quality_search_enabled_all_fields() {
        use dtvmgr_jlse::types::QualitySearchConfig;

        // Arrange
        let qs = QualitySearchConfig {
            enabled: true,
            target_vmaf: Some(95.0),
            max_encoded_percent: Some(70.0),
            min_vmaf_tolerance: None,
            thorough: Some(false),
            sample_duration_secs: Some(5.0),
            skip_secs: Some(60.0),
            sample_every_secs: Some(600.0),
            min_samples: Some(3),
            max_samples: Some(10),
            vmaf_subsample: Some(3),
        };

        // Act
        let mut out = String::new();
        AppConfig::write_quality_search(&mut out, Some(&qs));

        // Assert — section header is active (not commented)
        assert!(out.contains("[jlse.encode.quality_search]\n"));
        assert!(!out.contains("# [jlse.encode.quality_search]"));
        assert!(out.contains("enabled = true\n"));
        assert!(out.contains("target_vmaf = 95\n"));
        assert!(out.contains("vmaf_subsample = 3\n"));
        assert!(out.contains("sample_every_secs = 600\n"));
        assert!(out.contains("sample_duration_secs = 5\n"));
        assert!(out.contains("skip_secs = 60\n"));
        assert!(out.contains("min_samples = 3\n"));
        assert!(out.contains("max_samples = 10\n"));
        assert!(out.contains("max_encoded_percent = 70\n"));
        assert!(out.contains("thorough = false\n"));
    }

    #[test]
    fn test_quality_search_none_commented() {
        // Act
        let mut out = String::new();
        AppConfig::write_quality_search(&mut out, None);

        // Assert — entire section is commented out
        assert!(out.contains("# [jlse.encode.quality_search]"));
        assert!(out.contains("# enabled = true\n"));
        assert!(out.contains("# target_vmaf = 93.0\n"));
        assert!(out.contains("# vmaf_subsample = 5\n"));
        assert!(out.contains("# sample_every_secs = 720\n"));
    }

    #[test]
    fn test_quality_search_disabled_commented() {
        use dtvmgr_jlse::types::QualitySearchConfig;

        // Arrange — enabled = false falls into the `_` arm
        let qs = QualitySearchConfig {
            enabled: false,
            target_vmaf: Some(95.0),
            max_encoded_percent: None,
            min_vmaf_tolerance: None,
            thorough: None,
            sample_duration_secs: None,
            skip_secs: None,
            sample_every_secs: None,
            min_samples: None,
            max_samples: None,
            vmaf_subsample: None,
        };

        // Act
        let mut out = String::new();
        AppConfig::write_quality_search(&mut out, Some(&qs));

        // Assert — treated as commented (same as None)
        assert!(out.contains("# [jlse.encode.quality_search]"));
        assert!(out.contains("# enabled = true\n"));
    }

    #[test]
    fn test_quality_search_partial_fields() {
        use dtvmgr_jlse::types::QualitySearchConfig;

        // Arrange — enabled with only some fields set
        let qs = QualitySearchConfig {
            enabled: true,
            target_vmaf: Some(93.0),
            max_encoded_percent: None,
            min_vmaf_tolerance: None,
            thorough: None,
            sample_duration_secs: None,
            skip_secs: None,
            sample_every_secs: None,
            min_samples: None,
            max_samples: None,
            vmaf_subsample: None,
        };

        // Act
        let mut out = String::new();
        AppConfig::write_quality_search(&mut out, Some(&qs));

        // Assert — active section with mix of set and commented fields
        assert!(out.contains("[jlse.encode.quality_search]\n"));
        assert!(out.contains("enabled = true\n"));
        assert!(out.contains("target_vmaf = 93\n"));
        // Unset fields use commented hints
        assert!(out.contains("# max_encoded_percent = 80\n"));
        assert!(out.contains("# vmaf_subsample = 5\n"));
        assert!(out.contains("# sample_every_secs = 720\n"));
        assert!(out.contains("# sample_duration_secs = 3\n"));
        assert!(out.contains("# skip_secs = 120\n"));
        assert!(out.contains("# min_samples = 5\n"));
        assert!(out.contains("# max_samples = 15\n"));
        assert!(out.contains("# thorough = true\n"));
    }

    #[test]
    fn test_quality_search_roundtrip() {
        use dtvmgr_jlse::types::{EncodeAudio, EncodeInput, EncodeVideo, QualitySearchConfig};

        // Arrange
        let config = AppConfig {
            tmdb: TmdbConfig {
                language: Some(String::from("ja-JP")),
                ..TmdbConfig::default()
            },
            jlse: Some(JlseConfig {
                dirs: JlseDirs::default(),
                bins: JlseBins::default(),
                encode: Some(JlseEncode {
                    format: Some(String::from("mkv")),
                    input: Some(EncodeInput {
                        flags: Some(String::from("+discardcorrupt+genpts")),
                        analyzeduration: Some(String::from("30M")),
                        probesize: Some(String::from("100M")),
                        init_hw_device: None,
                        filter_hw_device: None,
                        hwaccel: None,
                        hwaccel_output_format: None,
                        decoder: None,
                    }),
                    video: Some(EncodeVideo {
                        codec: Some(String::from("libx264")),
                        preset: Some(String::from("medium")),
                        profile: Some(String::from("main")),
                        pix_fmt: Some(String::from("yuv420p")),
                        filter: None,
                        aspect: None,
                        extra: vec![],
                    }),
                    audio: Some(EncodeAudio {
                        codec: Some(String::from("aac")),
                        bitrate: Some(String::from("256k")),
                        channels: Some(2),
                        sample_rate: Some(48000),
                        extra: vec![],
                    }),
                    duration_check: None,
                    quality_search: Some(QualitySearchConfig {
                        enabled: true,
                        target_vmaf: Some(95.0),
                        max_encoded_percent: Some(70.0),
                        min_vmaf_tolerance: None,
                        thorough: Some(true),
                        sample_duration_secs: Some(5.0),
                        skip_secs: Some(60.0),
                        sample_every_secs: Some(600.0),
                        min_samples: Some(3),
                        max_samples: Some(10),
                        vmaf_subsample: Some(3),
                    }),
                }),
            }),
            ..AppConfig::default()
        };

        // Act — serialize and re-parse
        let toml_str = config.to_commented_toml();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();

        // Assert — quality_search fields survive roundtrip
        let qs = parsed
            .jlse
            .as_ref()
            .unwrap()
            .encode
            .as_ref()
            .unwrap()
            .quality_search
            .as_ref()
            .unwrap();
        assert!(qs.enabled);
        assert_eq!(qs.target_vmaf, Some(95.0));
        assert_eq!(qs.max_encoded_percent, Some(70.0));
        assert_eq!(qs.thorough, Some(true));
        assert_eq!(qs.sample_duration_secs, Some(5.0));
        assert_eq!(qs.skip_secs, Some(60.0));
        assert_eq!(qs.sample_every_secs, Some(600.0));
        assert_eq!(qs.min_samples, Some(3));
        assert_eq!(qs.max_samples, Some(10));
        assert_eq!(qs.vmaf_subsample, Some(3));
    }

    #[test]
    fn test_to_commented_toml_with_duration_check_rules() {
        // Arrange: config with custom duration_check rules
        let config = AppConfig {
            jlse: Some(JlseConfig {
                dirs: JlseDirs::default(),
                bins: JlseBins::default(),
                encode: Some(JlseEncode {
                    format: Some(String::from("mkv")),
                    input: None,
                    video: None,
                    audio: None,
                    duration_check: Some(vec![
                        DurationCheckRule {
                            min_min: 0,
                            max_min: 30,
                            min_percent: 60,
                        },
                        DurationCheckRule {
                            min_min: 31,
                            max_min: 120,
                            min_percent: 70,
                        },
                    ]),
                    quality_search: None,
                }),
            }),
            ..AppConfig::default()
        };

        // Act
        let toml_str = config.to_commented_toml();

        // Assert: active (non-commented) duration_check entries
        assert!(
            toml_str.contains("[[jlse.encode.duration_check]]"),
            "expected active duration_check entries in:\n{toml_str}"
        );
        assert!(toml_str.contains("min_percent = 60"));
        assert!(toml_str.contains("min_percent = 70"));

        // Roundtrip: parse back and verify
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();
        let rules = parsed.jlse.unwrap().encode.unwrap().duration_check.unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].min_min, 0);
        assert_eq!(rules[0].max_min, 30);
        assert_eq!(rules[0].min_percent, 60);
        assert_eq!(rules[1].min_min, 31);
        assert_eq!(rules[1].max_min, 120);
        assert_eq!(rules[1].min_percent, 70);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_nonexistent_creates_default() {
        // Arrange: path that doesn't exist
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");

        // Act
        let config = AppConfig::load(&path).unwrap();

        // Assert: default config is returned
        assert!(config.syoboi.channels.selected.is_empty());
        // File was created with default template
        assert!(path.exists());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_unreadable_path_error() {
        // Arrange: directory path (not a file) causes read error
        let dir = tempfile::tempdir().unwrap();
        // Use the directory itself as the config path
        let result = AppConfig::load(dir.path());

        // Assert: returns error
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(
            err.contains("failed to read"),
            "expected 'failed to read' in: {err}"
        );
    }

    #[test]
    fn test_to_commented_toml_with_hidden_storage_dirs() {
        // Arrange
        let config = AppConfig {
            epgstation: EpgStationConfig {
                hidden_storage_dirs: vec![String::from("/mnt/nas1"), String::from("/mnt/nas2")],
                ..EpgStationConfig::default()
            },
            ..AppConfig::default()
        };

        // Act
        let toml_str = config.to_commented_toml();

        // Assert: active (non-commented) hidden_storage_dirs entries
        assert!(
            toml_str.contains("hidden_storage_dirs"),
            "should contain hidden_storage_dirs key"
        );
        assert!(toml_str.contains("/mnt/nas1"));
        assert!(toml_str.contains("/mnt/nas2"));
    }
}
