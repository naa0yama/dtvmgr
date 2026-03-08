//! Broadcast channel detection from filenames.
//!
//! Parses `ChList.csv` and matches filenames against channel entries
//! using a priority-based algorithm ported from the original Node.js
//! `channel.js`.

use std::path::Path;

use anyhow::{Context, Result};
use regex::Regex;
use tracing::debug;
use unicode_normalization::UnicodeNormalization;

use crate::types::Channel;

/// Loads channel entries from a `ChList.csv` file.
///
/// The CSV is expected to have a header row (skipped) followed by data
/// rows with 4 columns: `recognize`, `install`, `short`, `service_id`.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_channels(csv_path: &Path) -> Result<Vec<Channel>> {
    let data = std::fs::read_to_string(csv_path)
        .with_context(|| format!("failed to read channel list: {}", csv_path.display()))?;

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(data.as_bytes());

    let mut channels = Vec::new();
    for (i, record) in reader.records().enumerate() {
        let record =
            record.with_context(|| format!("failed to parse channel record at line {i}"))?;
        if record.len() < 4 {
            continue;
        }
        channels.push(Channel {
            recognize: record.get(0).unwrap_or_default().to_owned(),
            install: record.get(1).unwrap_or_default().to_owned(),
            short: record.get(2).unwrap_or_default().to_owned(),
            service_id: record.get(3).unwrap_or_default().to_owned(),
        });
    }

    debug!(count = channels.len(), "loaded channel entries");
    Ok(channels)
}

/// Applies NFKC normalization (fullwidth alphanumeric to halfwidth,
/// halfwidth katakana to fullwidth).
fn normalize(s: &str) -> String {
    s.nfkc().collect()
}

/// Looks up a channel by a single service ID.
///
/// Compares the decimal string representation of `sid` against each
/// channel's `service_id` field. Returns the first match.
#[must_use]
pub fn lookup_channel_by_sid(channels: &[Channel], sid: u32) -> Option<Channel> {
    let sid_str = sid.to_string();
    channels.iter().find(|ch| ch.service_id == sid_str).cloned()
}

/// Looks up a channel by multiple service IDs.
///
/// Tries each SID in order and returns the first match found in
/// `channels`. Useful for multi-service TS files where the PAT
/// contains several service entries.
#[must_use]
pub fn lookup_channel_by_sids(channels: &[Channel], sids: &[u32]) -> Option<Channel> {
    sids.iter()
        .find_map(|sid| lookup_channel_by_sid(channels, *sid))
}

/// Detects the broadcast channel with optional PAT SID lookup.
///
/// Priority order:
/// 1. `channel_name` (explicit `--channel` flag)
/// 2. PAT SID → `ChList.csv` reverse lookup
/// 3. Filename pattern matching (fallback)
///
/// Returns `None` if no channel matches.
#[must_use]
#[allow(clippy::module_name_repetitions)]
pub fn detect_channel_with_sid(
    channels: &[Channel],
    filepath: &str,
    channel_name: Option<&str>,
    pat_sids: Option<&[u32]>,
) -> Option<Channel> {
    let filename = normalize(
        &Path::new(filepath)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
    );

    // Priority 1: explicit channel name
    if let Some(name) = channel_name
        && !name.is_empty()
    {
        let cn = normalize(name);
        if let Some(ch) = match_by_channel_name(channels, &cn) {
            return Some(ch);
        }
    }

    // Priority 2: PAT SID reverse lookup
    if let Some(sids) = pat_sids
        && let Some(ch) = lookup_channel_by_sids(channels, sids)
    {
        debug!(
            short = %ch.short,
            service_id = %ch.service_id,
            "detected channel via PAT SID"
        );
        return Some(ch);
    }

    // Priority 3: filename pattern matching
    match_by_filename(channels, &filename)
}

/// Detects the broadcast channel from a filename and channel list.
///
/// When `channel_name` is provided (from `--channel` flag or
/// `CHNNELNAME` env var), it takes priority. Falls back to filename
/// detection if the channel name does not match.
///
/// This is a convenience wrapper around [`detect_channel_with_sid`]
/// without PAT SID lookup.
///
/// Returns `None` if no channel matches.
#[must_use]
#[allow(clippy::module_name_repetitions)]
pub fn detect_channel(
    channels: &[Channel],
    filepath: &str,
    channel_name: Option<&str>,
) -> Option<Channel> {
    detect_channel_with_sid(channels, filepath, channel_name, None)
}

/// Matches channel by explicit channel name (priority search).
fn match_by_channel_name(channels: &[Channel], channel_name: &str) -> Option<Channel> {
    for ch in channels {
        let recognize = normalize(&ch.recognize);
        let short = normalize(&ch.short);
        let service_id = &ch.service_id;

        // recognize: prefix match
        if channel_name.starts_with(&recognize) {
            return Some(ch.clone());
        }

        // short: prefix match
        if channel_name.starts_with(&short) {
            return Some(ch.clone());
        }

        // service_id: prefix match
        if channel_name.starts_with(service_id) {
            return Some(ch.clone());
        }

        // recognize: remove single isolated digits (not at end) and retry
        let without_digits = remove_isolated_digits(channel_name);
        if without_digits != channel_name && without_digits.starts_with(&recognize) {
            return Some(ch.clone());
        }
    }
    None
}

/// Removes single digits that are not adjacent to other digits
/// and not at the end of the string.
///
/// Equivalent to the JS pattern:
/// `channelName.replace(/(?<!\d)\d(?!\d|$)/g, "")`
fn remove_isolated_digits(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut result = String::with_capacity(s.len());

    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_digit() {
            let prev_is_digit = i
                .checked_sub(1)
                .is_some_and(|p| chars.get(p).is_some_and(char::is_ascii_digit));
            let next_is_digit = chars
                .get(i.saturating_add(1))
                .is_some_and(char::is_ascii_digit);
            let is_at_end = i.saturating_add(1) == len;

            if !prev_is_digit && !next_is_digit && !is_at_end {
                continue; // skip this isolated digit
            }
        }
        result.push(c);
    }
    result
}

/// Open bracket characters used in filename patterns.
const OPEN_BRACKETS: &str = "(〔[{〈《｢『【≪";

/// Builds a regex character class string from bracket chars.
///
/// Escapes `[`, `]`, and `\` which are special inside character classes
/// in the Rust regex crate.
fn bracket_class(brackets: &str) -> String {
    let mut class = String::from("[");
    for c in brackets.chars() {
        if c == '[' || c == ']' || c == '\\' {
            class.push('\\');
        }
        class.push(c);
    }
    class.push(']');
    class
}

/// Close bracket + space/underscore character class for regex patterns.
const CLOSE_SEP_CLASS: &str = r"[)〕\]}〉》｣』】≫ _]";

/// Matches channel by filename using priority-based algorithm.
fn match_by_filename(channels: &[Channel], filename: &str) -> Option<Channel> {
    let open = bracket_class(OPEN_BRACKETS);

    let mut best_result: Option<Channel> = None;
    let mut best_priority: u32 = 0;

    for ch in channels {
        let recognize = normalize(&ch.recognize);
        let short = normalize(&ch.short);
        let service_id = &ch.service_id;

        // Priority 1: recognize at start or after " _"
        let pat = [&*format!("^{recognize}"), &*format!(" _{recognize}")].join("|");
        if try_regex_match(filename, &pat) {
            return Some(ch.clone());
        }

        // Priority 1: short at start/after_/after bracket, followed by space/bracket/_
        let pat = [
            &*format!("^{short}[_ ]"),
            &*format!(" _{short}"),
            &*format!(" {open}{short}{CLOSE_SEP_CLASS}"),
        ]
        .join("|");
        if try_regex_match(filename, &pat) {
            return Some(ch.clone());
        }

        // Priority 1: service_id same pattern as short
        let pat = [
            &*format!("^{service_id}[_ ]"),
            &*format!(" _{service_id}"),
            &*format!(" {open}{service_id}{CLOSE_SEP_CLASS}"),
        ]
        .join("|");
        if try_regex_match(filename, &pat) {
            return Some(ch.clone());
        }

        // Priority 2: recognize after open bracket
        if best_priority < 2 && try_regex_match(filename, &format!("{open}{recognize}")) {
            best_result = Some(ch.clone());
            best_priority = 2;
            continue;
        }

        // Priority 3: short surrounded by space/_ and bracket/space/_
        let pat = format!("[ _]{short}{CLOSE_SEP_CLASS}");
        if best_priority < 3 && try_regex_match(filename, &pat) {
            best_result = Some(ch.clone());
            best_priority = 3;
            continue;
        }

        // Priority 3: service_id same pattern
        let pat = format!("[ _]{service_id}{CLOSE_SEP_CLASS}");
        if best_priority < 3 && try_regex_match(filename, &pat) {
            best_result = Some(ch.clone());
            best_priority = 3;
            continue;
        }

        // Priority 4: recognize after _ or space
        let pat = [&*format!("_{recognize}"), &*format!(" {recognize}")].join("|");
        if best_priority < 4 && try_regex_match(filename, &pat) {
            best_result = Some(ch.clone());
            best_priority = 4;
        }
    }

    best_result
}

/// Attempts a regex match, returning false on regex compilation error.
fn try_regex_match(text: &str, pattern: &str) -> bool {
    match Regex::new(pattern) {
        Ok(re) => re.is_match(text),
        Err(e) => {
            debug!(pattern, error = %e, "regex compilation failed, skipping");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn sample_channels() -> Vec<Channel> {
        vec![
            Channel {
                recognize: String::from("ＮＨＫ総合"),
                install: String::new(),
                short: String::from("NHK-G"),
                service_id: String::from("1024"),
            },
            Channel {
                recognize: String::from("ＢＳ１１イレブン"),
                install: String::new(),
                short: String::from("BS11"),
                service_id: String::from("211"),
            },
            Channel {
                recognize: String::from("ＴＯＫＹＯ　ＭＸ"),
                install: String::new(),
                short: String::from("MX"),
                service_id: String::from("23608"),
            },
            Channel {
                recognize: String::from("ＡＴ−Ｘ"),
                install: String::new(),
                short: String::from("AT-X"),
                service_id: String::from("333"),
            },
        ]
    }

    #[cfg_attr(miri, ignore)] // regex DFA compilation is prohibitively slow under Miri
    #[test]
    fn test_detect_by_short_in_brackets() {
        // Arrange
        let channels = sample_channels();

        // Act: space before bracket (matches original JS regex pattern)
        let result = detect_channel(&channels, "番組名 [BS11]第1話.ts", None);

        // Assert
        assert_eq!(result.unwrap().short, "BS11");
    }

    #[cfg_attr(miri, ignore)] // regex DFA compilation is prohibitively slow under Miri
    #[test]
    fn test_detect_by_short_at_start() {
        // Arrange
        let channels = sample_channels();

        // Act: short code at start followed by underscore
        let result = detect_channel(&channels, "NHK-G 番組名.ts", None);

        // Assert
        assert_eq!(result.unwrap().short, "NHK-G");
    }

    #[cfg_attr(miri, ignore)] // regex DFA compilation is prohibitively slow under Miri
    #[test]
    fn test_detect_by_short_at_start_underscore() {
        // Arrange
        let channels = sample_channels();

        // Act: short code at start followed by underscore
        let result = detect_channel(&channels, "BS11_番組名.ts", None);

        // Assert
        assert_eq!(result.unwrap().short, "BS11");
    }

    #[cfg_attr(miri, ignore)] // regex DFA compilation is prohibitively slow under Miri
    #[test]
    fn test_detect_by_channel_name() {
        // Arrange
        let channels = sample_channels();

        // Act
        let result = detect_channel(&channels, "something.ts", Some("BS11"));

        // Assert
        assert_eq!(result.unwrap().short, "BS11");
    }

    #[cfg_attr(miri, ignore)] // regex DFA compilation is prohibitively slow under Miri
    #[test]
    fn test_detect_by_channel_name_recognize() {
        // Arrange
        let channels = sample_channels();

        // Act (full-width input normalized to match)
        let result = detect_channel(&channels, "something.ts", Some("NHK総合"));

        // Assert
        assert_eq!(result.unwrap().short, "NHK-G");
    }

    #[cfg_attr(miri, ignore)] // regex DFA compilation is prohibitively slow under Miri
    #[test]
    fn test_detect_by_service_id_in_brackets() {
        // Arrange
        let channels = sample_channels();

        // Act: space before bracket
        let result = detect_channel(&channels, "番組名 [211]第1話.ts", None);

        // Assert
        assert_eq!(result.unwrap().short, "BS11");
    }

    #[cfg_attr(miri, ignore)] // regex DFA compilation is prohibitively slow under Miri
    #[test]
    fn test_detect_recognize_after_bracket() {
        // Arrange
        let channels = sample_channels();

        // Act
        let result = detect_channel(&channels, "番組名【TOKYO MX】.ts", None);

        // Assert
        assert_eq!(result.unwrap().short, "MX");
    }

    #[cfg_attr(miri, ignore)] // regex DFA compilation is prohibitively slow under Miri
    #[test]
    fn test_detect_no_match() {
        // Arrange
        let channels = sample_channels();

        // Act
        let result = detect_channel(&channels, "unknown_channel_file.ts", None);

        // Assert
        assert!(result.is_none());
    }

    #[cfg_attr(miri, ignore)] // regex DFA compilation is prohibitively slow under Miri
    #[test]
    fn test_detect_channel_name_fallback_to_filename() {
        // Arrange
        let channels = sample_channels();

        // Act: channel name doesn't match, but filename has the short code at start
        let result = detect_channel(&channels, "AT-X_番組名.ts", Some("UnknownChannel"));

        // Assert
        assert_eq!(result.unwrap().short, "AT-X");
    }

    // ── lookup_channel_by_sid ─────────────────────────────

    #[test]
    fn test_lookup_channel_by_sid_match() {
        // Arrange
        let channels = sample_channels();

        // Act
        let result = lookup_channel_by_sid(&channels, 1024);

        // Assert
        assert_eq!(result.unwrap().short, "NHK-G");
    }

    #[test]
    fn test_lookup_channel_by_sid_no_match() {
        // Arrange
        let channels = sample_channels();

        // Act
        let result = lookup_channel_by_sid(&channels, 9999);

        // Assert
        assert!(result.is_none());
    }

    // ── lookup_channel_by_sids ────────────────────────────

    #[test]
    fn test_lookup_channel_by_sids_first_match() {
        // Arrange
        let channels = sample_channels();
        let sids = vec![211, 1024]; // BS11 matches first

        // Act
        let result = lookup_channel_by_sids(&channels, &sids);

        // Assert
        assert_eq!(result.unwrap().short, "BS11");
    }

    #[test]
    fn test_lookup_channel_by_sids_second_match() {
        // Arrange
        let channels = sample_channels();
        let sids = vec![9999, 23608]; // first doesn't match, MX matches

        // Act
        let result = lookup_channel_by_sids(&channels, &sids);

        // Assert
        assert_eq!(result.unwrap().short, "MX");
    }

    #[test]
    fn test_lookup_channel_by_sids_empty() {
        // Arrange
        let channels = sample_channels();

        // Act
        let result = lookup_channel_by_sids(&channels, &[]);

        // Assert
        assert!(result.is_none());
    }

    // ── detect_channel_with_sid ───────────────────────────

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_detect_channel_with_sid_channel_name_priority() {
        // Arrange
        let channels = sample_channels();
        let sids = vec![211]; // BS11

        // Act — channel_name should win over PAT SID
        let result = detect_channel_with_sid(&channels, "something.ts", Some("AT-X"), Some(&sids));

        // Assert
        assert_eq!(result.unwrap().short, "AT-X");
    }

    #[test]
    fn test_detect_channel_with_sid_pat_sid() {
        // Arrange
        let channels = sample_channels();
        let sids = vec![23608]; // MX

        // Act — no channel name, PAT SID should match
        let result = detect_channel_with_sid(&channels, "unknown_file.ts", None, Some(&sids));

        // Assert
        assert_eq!(result.unwrap().short, "MX");
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn test_detect_channel_with_sid_fallback_to_filename() {
        // Arrange
        let channels = sample_channels();
        let sids = vec![9999]; // no match

        // Act — PAT SID doesn't match, falls back to filename
        let result = detect_channel_with_sid(&channels, "BS11_番組名.ts", None, Some(&sids));

        // Assert
        assert_eq!(result.unwrap().short, "BS11");
    }

    #[test]
    fn test_detect_channel_with_sid_none_sids() {
        // Arrange
        let channels = sample_channels();

        // Act — None for pat_sids, equivalent to detect_channel
        let result = detect_channel_with_sid(&channels, "unknown_file.ts", None, None);

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn test_normalize_nfkc() {
        // Arrange & Act
        let result = normalize("ＡＢＣ１２３");

        // Assert
        assert_eq!(result, "ABC123");
    }

    #[test]
    fn test_remove_isolated_digits() {
        // Arrange & Act & Assert
        assert_eq!(remove_isolated_digits("abc1def"), "abcdef");
        assert_eq!(remove_isolated_digits("abc12def"), "abc12def");
        assert_eq!(remove_isolated_digits("abc1"), "abc1"); // at end, keep
        assert_eq!(remove_isolated_digits("1abc"), "abc"); // isolated, not at end
    }

    #[test]
    #[cfg_attr(miri, ignore)] // tempdir requires mkdir, unsupported under Miri isolation
    fn test_load_channels_from_csv() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let csv_path = dir.path().join("ChList.csv");
        std::fs::write(
            &csv_path,
            "放送局名（認識用）,放送局名（設定用）,略称,サービスID\n\
             ＮＨＫＢＳ１,,BS1,101\n\
             ＢＳ１１イレブン,,BS11,211\n",
        )
        .unwrap();

        // Act
        let channels = load_channels(&csv_path).unwrap();

        // Assert
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].short, "BS1");
        assert_eq!(channels[0].service_id, "101");
        assert_eq!(channels[1].short, "BS11");
    }
}
