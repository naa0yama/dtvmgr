//! JL parameter detection from channel and filename.
//!
//! Parses `ChParamJL1.csv` / `ChParamJL2.csv` and matches against the
//! detected channel and input filename to determine which JL command
//! file and flags to use. Ported from the original Node.js `param.js`.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use regex::Regex;
use tracing::{debug, instrument};
use unicode_normalization::UnicodeNormalization;

use crate::types::{Channel, DetectionParam, Param};

/// Characters that indicate a regex pattern in the title field.
const REGEX_META_CHARS: &[char] = &['.', '*', '+', '?', '|', '[', ']', '^'];

/// Loads parameter entries from a `ChParamJL*.csv` file.
///
/// The CSV is expected to have a header row (skipped) followed by data
/// rows with 7 columns. Rows where the channel column starts with `#`
/// are treated as comments and **retained** for potential default
/// fallback but skipped during matching.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
#[instrument(skip_all, err(level = "error"))]
pub fn load_params(csv_path: &Path) -> Result<Vec<Param>> {
    let data = std::fs::read_to_string(csv_path)
        .with_context(|| format!("failed to read param list: {}", csv_path.display()))?;

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(data.as_bytes());

    let mut params = Vec::new();
    for (i, record) in reader.records().enumerate() {
        let record = record.with_context(|| {
            format!(
                "failed to parse param record at line {}",
                i.saturating_add(2)
            )
        })?;
        // Ensure at least 3 columns (channel, title, jl_run).
        if record.len() < 3 {
            continue;
        }
        params.push(Param {
            channel: record.get(0).unwrap_or_default().to_owned(),
            title: record.get(1).unwrap_or_default().to_owned(),
            jl_run: record.get(2).unwrap_or_default().to_owned(),
            flags: record.get(3).unwrap_or_default().to_owned(),
            options: record.get(4).unwrap_or_default().to_owned(),
            comment_view: record.get(5).unwrap_or_default().to_owned(),
            comment: record.get(6).unwrap_or_default().to_owned(),
        });
    }

    debug!(count = params.len(), path = %csv_path.display(), "loaded param entries");
    Ok(params)
}

/// Detects JL parameters by matching channel and filename.
///
/// Searches `params_jl1` first, then `params_jl2`, merging results.
/// Fields from JL2 overwrite JL1 (equivalent to JS `Object.assign`).
#[instrument(skip_all)]
#[must_use]
#[allow(clippy::module_name_repetitions)]
pub fn detect_param(
    params_jl1: &[Param],
    params_jl2: &[Param],
    channel: Option<&Channel>,
    filename: &str,
) -> DetectionParam {
    let mut merged: HashMap<String, String> = HashMap::new();

    // Search JL1, then JL2 (JL2 overwrites JL1).
    for params in [params_jl1, params_jl2] {
        let result = search_params(params, channel, filename);
        for (key, value) in result {
            if value == "@" {
                merged.insert(key, String::new());
            } else if !value.is_empty() {
                merged.insert(key, value);
            }
        }
    }

    DetectionParam {
        jl_run: merged.remove("jl_run").unwrap_or_default(),
        flags: merged.remove("flags").unwrap_or_default(),
        options: merged.remove("options").unwrap_or_default(),
    }
}

/// Searches a single param list for matching entries.
///
/// Returns a map of field name to value. The `@` sentinel is
/// preserved so the caller can perform the clear logic.
fn search_params(
    params: &[Param],
    channel: Option<&Channel>,
    filename: &str,
) -> HashMap<String, String> {
    let short = channel.map_or("__normal", |ch| ch.short.as_str());
    let normal_filename: String = filename.nfkc().collect();

    let mut result: HashMap<String, String> = HashMap::new();

    for param in params {
        // Skip comment rows.
        if param.channel.starts_with('#') {
            continue;
        }

        let channel_matches = short == param.channel;
        let has_title = !param.title.is_empty();

        let is_match = if channel_matches && has_title {
            match_title(&normal_filename, &param.title)
        } else {
            channel_matches
        };

        if is_match {
            merge_param_fields(&mut result, param);
        }
    }

    // If nothing matched, use the first non-comment row as default.
    if result.is_empty()
        && let Some(default_param) = params.iter().find(|p| !p.channel.starts_with('#'))
    {
        merge_param_fields(&mut result, default_param);
    }

    result
}

/// Matches the title pattern against the normalized filename.
///
/// If the title contains regex metacharacters, it is treated as a
/// regex pattern. Otherwise, a simple substring match is performed.
fn match_title(normalized_filename: &str, title: &str) -> bool {
    let normal_title: String = title.nfkc().collect();

    if title.contains(REGEX_META_CHARS) {
        // Regex match
        match Regex::new(&normal_title) {
            Ok(re) => re.is_match(normalized_filename),
            Err(e) => {
                debug!(title, error = %e, "title regex compilation failed");
                false
            }
        }
    } else {
        // Substring match
        normalized_filename.contains(&normal_title)
    }
}

/// Merges a param's fields into the result map.
///
/// - `@` is stored as-is (caller handles clearing).
/// - Empty values are skipped (do not overwrite existing values).
/// - Non-empty values overwrite existing values.
fn merge_param_fields(result: &mut HashMap<String, String>, param: &Param) {
    let fields = [
        ("jl_run", &param.jl_run),
        ("flags", &param.flags),
        ("options", &param.options),
    ];

    for (key, value) in fields {
        if *value == "@" {
            result.insert(String::from(key), String::from("@"));
        } else if !value.is_empty() {
            result.insert(String::from(key), (*value).clone());
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn default_param() -> Param {
        Param {
            channel: String::new(),
            title: String::new(),
            jl_run: String::from("JL_standard.txt"),
            flags: String::from("@"),
            options: String::from("@"),
            comment_view: String::new(),
            comment: String::from("default"),
        }
    }

    fn nhk_param() -> Param {
        Param {
            channel: String::from("NHK-G"),
            title: String::new(),
            jl_run: String::from("JL_NHK.txt"),
            flags: String::new(),
            options: String::new(),
            comment_view: String::from("NHK settings"),
            comment: String::new(),
        }
    }

    fn wowow_param() -> Param {
        Param {
            channel: String::from("WOWOW"),
            title: String::new(),
            jl_run: String::from("JL_standard.txt"),
            flags: String::from("fLOff,fHCWOWA"),
            options: String::new(),
            comment_view: String::new(),
            comment: String::new(),
        }
    }

    fn animax_title_param() -> Param {
        Param {
            channel: String::from("ANIMAX"),
            title: String::from("特別番組.*"),
            jl_run: String::from("JL_special.txt"),
            flags: String::new(),
            options: String::new(),
            comment_view: String::new(),
            comment: String::new(),
        }
    }

    fn nhk_channel() -> Channel {
        Channel {
            recognize: String::from("ＮＨＫ総合"),
            install: String::new(),
            short: String::from("NHK-G"),
            service_id: String::from("1024"),
        }
    }

    #[test]
    fn test_detect_nhk_channel_match() {
        // Arrange
        let jl1 = vec![default_param(), nhk_param()];
        let jl2: Vec<Param> = vec![];
        let channel = nhk_channel();

        // Act
        let result = detect_param(&jl1, &jl2, Some(&channel), "番組名");

        // Assert
        assert_eq!(result.jl_run, "JL_NHK.txt");
        assert!(result.flags.is_empty());
        assert!(result.options.is_empty());
    }

    #[test]
    fn test_detect_default_fallback() {
        // Arrange
        let jl1 = vec![default_param(), nhk_param()];
        let jl2: Vec<Param> = vec![];

        // Act: no channel provided, should fallback to default
        let result = detect_param(&jl1, &jl2, None, "番組名");

        // Assert
        assert_eq!(result.jl_run, "JL_standard.txt");
        assert!(result.flags.is_empty()); // "@" clears to empty
        assert!(result.options.is_empty()); // "@" clears to empty
    }

    #[test]
    fn test_detect_jl2_overwrites_jl1() {
        // Arrange
        let jl1 = vec![default_param(), nhk_param()];
        let jl2 = vec![Param {
            channel: String::from("NHK-G"),
            title: String::new(),
            jl_run: String::from("JL_NHK_v2.txt"),
            flags: String::from("customFlag"),
            options: String::new(),
            comment_view: String::new(),
            comment: String::new(),
        }];
        let channel = nhk_channel();

        // Act
        let result = detect_param(&jl1, &jl2, Some(&channel), "番組名");

        // Assert: JL2 values overwrite JL1
        assert_eq!(result.jl_run, "JL_NHK_v2.txt");
        assert_eq!(result.flags, "customFlag");
    }

    #[cfg_attr(miri, ignore)] // regex DFA compilation is prohibitively slow under Miri
    #[test]
    fn test_detect_with_title_regex() {
        // Arrange
        let jl1 = vec![default_param(), animax_title_param()];
        let jl2: Vec<Param> = vec![];
        let channel = Channel {
            recognize: String::new(),
            install: String::new(),
            short: String::from("ANIMAX"),
            service_id: String::from("670"),
        };

        // Act: title regex "特別番組.*" matches filename
        let result = detect_param(&jl1, &jl2, Some(&channel), "特別番組スペシャル");

        // Assert
        assert_eq!(result.jl_run, "JL_special.txt");
    }

    #[cfg_attr(miri, ignore)] // regex DFA compilation is prohibitively slow under Miri
    #[test]
    fn test_detect_title_regex_no_match() {
        // Arrange
        let jl1 = vec![default_param(), animax_title_param()];
        let jl2: Vec<Param> = vec![];
        let channel = Channel {
            recognize: String::new(),
            install: String::new(),
            short: String::from("ANIMAX"),
            service_id: String::from("670"),
        };

        // Act: title regex "特別番組.*" does NOT match filename
        let result = detect_param(&jl1, &jl2, Some(&channel), "通常番組");

        // Assert: falls through to default
        assert_eq!(result.jl_run, "JL_standard.txt");
    }

    #[test]
    fn test_at_marker_clears_field() {
        // Arrange
        let jl1 = vec![wowow_param()];
        let jl2 = vec![Param {
            channel: String::from("WOWOW"),
            title: String::new(),
            jl_run: String::new(),    // empty = keep existing
            flags: String::from("@"), // "@" = clear
            options: String::new(),
            comment_view: String::new(),
            comment: String::new(),
        }];
        let channel = Channel {
            recognize: String::new(),
            install: String::new(),
            short: String::from("WOWOW"),
            service_id: String::from("191"),
        };

        // Act
        let result = detect_param(&jl1, &jl2, Some(&channel), "番組名");

        // Assert: flags cleared by "@" in JL2
        assert_eq!(result.jl_run, "JL_standard.txt");
        assert!(result.flags.is_empty());
    }

    #[test]
    fn test_comment_row_skipped() {
        // Arrange
        let jl1 = vec![
            default_param(),
            Param {
                channel: String::from("#NHK-G"),
                title: String::new(),
                jl_run: String::from("should_not_match.txt"),
                flags: String::new(),
                options: String::new(),
                comment_view: String::new(),
                comment: String::new(),
            },
        ];
        let jl2: Vec<Param> = vec![];
        let channel = nhk_channel();

        // Act
        let result = detect_param(&jl1, &jl2, Some(&channel), "番組名");

        // Assert: comment row is skipped, falls back to default
        assert_eq!(result.jl_run, "JL_standard.txt");
    }

    #[test]
    #[cfg_attr(miri, ignore)] // tempdir requires mkdir, unsupported under Miri isolation
    fn test_load_params_from_csv() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let csv_path = dir.path().join("ChParamJL1.csv");
        std::fs::write(
            &csv_path,
            "放送局略称,タイトル,JL_RUN,FLAGS,OPTIONS,#コメント表示用,#コメント\n\
             ,,JL_standard.txt,@,@,,default\n\
             NHK-G,,JL_NHK.txt,,,NHK settings,\n",
        )
        .unwrap();

        // Act
        let params = load_params(&csv_path).unwrap();

        // Assert
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].jl_run, "JL_standard.txt");
        assert_eq!(params[0].flags, "@");
        assert_eq!(params[1].channel, "NHK-G");
        assert_eq!(params[1].jl_run, "JL_NHK.txt");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_params_skips_short_records() {
        // Arrange: CSV with a short record (< 3 columns)
        let dir = tempfile::tempdir().unwrap();
        let csv_path = dir.path().join("ChParamJL1.csv");
        std::fs::write(
            &csv_path,
            "放送局略称,タイトル,JL_RUN,FLAGS,OPTIONS,#コメント表示用,#コメント\n\
             ,,JL_standard.txt,@,@,,default\n\
             Short,Only\n\
             NHK-G,,JL_NHK.txt,,,NHK settings,\n",
        )
        .unwrap();

        // Act
        let params = load_params(&csv_path).unwrap();

        // Assert — short record skipped
        assert_eq!(params.len(), 2);
    }

    // ── match_title edge cases ─────────────────────────────

    #[test]
    fn test_match_title_invalid_regex() {
        // Arrange: title contains regex metachar but is invalid regex
        let result = match_title("test_file.ts", "[invalid(regex");

        // Assert — returns false on regex compilation error
        assert!(!result);
    }

    #[test]
    fn test_match_title_substring() {
        // Arrange: title without regex metachar → substring match
        let result = match_title("特別番組テスト.ts", "番組テスト");

        // Assert
        assert!(result);
    }

    #[test]
    fn test_match_title_substring_no_match() {
        // Arrange: title without regex metachar → no substring match
        let result = match_title("別の番組.ts", "特別番組");

        // Assert
        assert!(!result);
    }
}
