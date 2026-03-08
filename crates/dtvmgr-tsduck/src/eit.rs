//! EIT (Event Information Table) XML parser.
//!
//! Parses `TSDuck` `tstables` XML output into structured program information.
//! Supports both decimal and hexadecimal `service_id` formats.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Root element of `TSDuck` XML output.
#[derive(Debug, Deserialize)]
pub struct TsduckXml {
    /// EIT tables in the XML.
    #[serde(rename = "EIT", default)]
    pub eit_tables: Vec<Table>,
}

/// A single EIT table.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Deserialize)]
pub struct Table {
    /// Service ID (decimal or hex like `0xFE00`).
    #[serde(rename = "@service_id")]
    pub service_id: String,
    /// Transport stream ID.
    #[serde(rename = "@transport_stream_id", default)]
    pub transport_stream_id: Option<String>,
    /// Original network ID.
    #[serde(rename = "@original_network_id", default)]
    pub original_network_id: Option<String>,
    /// EIT table type (e.g. `"pf"` for present/following, `"schedule"` for schedule).
    #[serde(rename = "@type", default)]
    pub table_type: Option<String>,
    /// Events within this EIT table.
    #[serde(rename = "event", default)]
    pub events: Vec<Event>,
}

/// A single event within an EIT table.
#[derive(Debug, Deserialize)]
pub struct Event {
    /// Event ID.
    #[serde(rename = "@event_id")]
    pub event_id: String,
    /// Start time (e.g. `"2024-12-31 15:00:00"`).
    #[serde(rename = "@start_time")]
    pub start_time: String,
    /// Duration in `HH:MM:SS` format.
    #[serde(rename = "@duration")]
    pub duration: String,
    /// Running status (e.g. `"running"`, `"not-running"`).
    #[serde(rename = "@running_status", default)]
    pub running_status: Option<String>,
    /// Short event descriptors.
    #[serde(rename = "short_event_descriptor", default)]
    pub short_event_descriptors: Vec<ShortEventDescriptor>,
    /// Extended event descriptors.
    #[serde(rename = "extended_event_descriptor", default)]
    pub extended_event_descriptors: Vec<ExtendedEventDescriptor>,
    /// Content descriptors (genre classification).
    #[serde(rename = "content_descriptor", default)]
    pub content_descriptors: Vec<ContentDescriptor>,
    /// Component descriptors (video attributes).
    #[serde(rename = "component_descriptor", default)]
    pub component_descriptors: Vec<ComponentDescriptor>,
    /// Audio component descriptors (ISDB audio attributes).
    #[serde(rename = "audio_component_descriptor", default)]
    pub audio_component_descriptors: Vec<AudioComponentDescriptor>,
}

/// Short event descriptor containing program name and description.
#[derive(Debug, Deserialize)]
pub struct ShortEventDescriptor {
    /// Language code (e.g. `"jpn"`).
    #[serde(rename = "@language_code", default)]
    pub language_code: Option<String>,
    /// Program name.
    #[serde(default)]
    pub event_name: Option<String>,
    /// Program description text.
    #[serde(default)]
    pub text: Option<String>,
}

/// Extended event descriptor containing detailed program information.
#[derive(Debug, Deserialize)]
pub struct ExtendedEventDescriptor {
    /// Descriptor sequence number.
    #[serde(rename = "@descriptor_number", default)]
    pub descriptor_number: Option<u8>,
    /// Last descriptor number in sequence.
    #[serde(rename = "@last_descriptor_number", default)]
    pub last_descriptor_number: Option<u8>,
    /// Language code (e.g. `"jpn"`).
    #[serde(rename = "@language_code", default)]
    pub language_code: Option<String>,
    /// Extended event items (key-value pairs).
    #[serde(rename = "item", default)]
    pub items: Vec<ExtendedEventItem>,
    /// Freeform text.
    #[serde(default)]
    pub text: Option<String>,
}

/// A single item within an extended event descriptor.
#[derive(Debug, Deserialize)]
pub struct ExtendedEventItem {
    /// Item description key (e.g. `"出演者"`).
    #[serde(default)]
    pub description: Option<String>,
    /// Item value text.
    #[serde(default)]
    pub name: Option<String>,
}

/// Content descriptor for genre classification.
#[derive(Debug, Deserialize)]
pub struct ContentDescriptor {
    /// Content entries.
    #[serde(rename = "content", default)]
    pub contents: Vec<ContentEntry>,
}

/// A single content classification entry.
#[derive(Debug, Deserialize)]
pub struct ContentEntry {
    /// Major genre (content nibble level 1).
    #[serde(rename = "@content_nibble_level_1", default)]
    pub content_nibble_level_1: Option<u8>,
    /// Sub-genre (content nibble level 2).
    #[serde(rename = "@content_nibble_level_2", default)]
    pub content_nibble_level_2: Option<u8>,
}

/// Component descriptor for video stream attributes.
#[derive(Debug, Deserialize)]
pub struct ComponentDescriptor {
    /// Stream content type (decimal or hex string).
    #[serde(rename = "@stream_content", default)]
    pub stream_content: Option<String>,
    /// Component type (decimal or hex string, encodes resolution).
    #[serde(rename = "@component_type", default)]
    pub component_type: Option<String>,
    /// Language code.
    #[serde(rename = "@language_code", default)]
    pub language_code: Option<String>,
    /// Descriptive text.
    #[serde(default)]
    pub text: Option<String>,
}

/// Audio component descriptor (ISDB).
#[derive(Debug, Deserialize)]
pub struct AudioComponentDescriptor {
    /// Stream content type (decimal or hex string).
    #[serde(rename = "@stream_content", default)]
    pub stream_content: Option<String>,
    /// Audio component type (decimal or hex string).
    #[serde(rename = "@component_type", default)]
    pub component_type: Option<String>,
    /// Sampling rate code (ARIB STD-B10).
    #[serde(rename = "@sampling_rate", default)]
    pub sampling_rate: Option<u8>,
    /// Language code (`TSDuck` uses `ISO_639_language_code` for this descriptor).
    #[serde(alias = "@language_code", rename = "@ISO_639_language_code", default)]
    pub language_code: Option<String>,
}

/// Parsed program information extracted from EIT data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramInfo {
    /// Service ID (numeric).
    pub service_id: u32,
    /// Event ID string.
    pub event_id: String,
    /// Start time string.
    pub start_time: String,
    /// Duration in seconds.
    pub duration_sec: u32,
    /// Raw duration string (`HH:MM:SS`).
    pub duration_raw: String,
    /// Running status.
    pub running_status: String,
    /// Program name from short event descriptor.
    pub program_name: Option<String>,
    /// Program description from short event descriptor.
    pub description: Option<String>,
    /// EIT table type (`"pf"` or `"schedule"`).
    pub table_type: Option<String>,
    /// Raw extended event key-value pairs (insertion order preserved).
    pub raw_extended: Vec<(String, String)>,
    /// Major genre code (content nibble level 1).
    pub genre1: Option<u8>,
    /// Sub-genre code (content nibble level 2).
    pub sub_genre1: Option<u8>,
    /// Video stream content type.
    pub video_stream_content: Option<u8>,
    /// Video component type.
    pub video_component_type: Option<u8>,
    /// Audio component type.
    pub audio_component_type: Option<u8>,
    /// Audio sampling rate code (ARIB STD-B10).
    pub audio_sampling_rate_code: Option<u8>,
}

impl ProgramInfo {
    /// Duration in minutes (truncated from seconds).
    #[must_use]
    pub const fn duration_min(&self) -> u32 {
        self.duration_sec / 60
    }

    /// Decoded video resolution string (e.g. `"1080i"`).
    ///
    /// Derived from `video_component_type` using ARIB STD-B10 table.
    #[must_use]
    pub fn video_resolution(&self) -> Option<&'static str> {
        self.video_component_type.and_then(decode_video_resolution)
    }

    /// Decoded audio sampling rate in Hz.
    ///
    /// Derived from `audio_sampling_rate_code` using ARIB STD-B10 table.
    #[must_use]
    pub fn audio_sampling_rate(&self) -> Option<u32> {
        self.audio_sampling_rate_code.and_then(decode_sampling_rate)
    }

    /// Concatenated extended event text in EPGStation-compatible format.
    ///
    /// Derived from `raw_extended` key-value pairs, prefixing each key with `◇`.
    #[must_use]
    pub fn extended(&self) -> Option<String> {
        if self.raw_extended.is_empty() {
            return None;
        }
        let mut out = String::new();
        for (i, (key, value)) in self.raw_extended.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            if !key.starts_with('◇') {
                out.push('◇');
            }
            out.push_str(key);
            out.push('\n');
            out.push_str(value);
        }
        Some(out)
    }
}

/// Parse all EIT events from `TSDuck` XML output.
///
/// # Errors
///
/// Returns an error if the XML is malformed or service IDs cannot be parsed.
pub fn parse_eit_xml(xml: &str) -> Result<Vec<ProgramInfo>> {
    let doc: TsduckXml = quick_xml::de::from_str(xml).context("failed to parse TSDuck EIT XML")?;

    let mut programs = Vec::new();
    for table in doc.eit_tables {
        let sid = parse_sid(&table.service_id)
            .with_context(|| format!("invalid service_id: {}", table.service_id))?;
        let table_type = table.table_type;
        for event in table.events {
            let duration_sec = parse_duration_to_sec(&event.duration).unwrap_or(0);
            let program_name = event
                .short_event_descriptors
                .first()
                .and_then(|d| d.event_name.clone());
            let description = event
                .short_event_descriptors
                .first()
                .and_then(|d| d.text.clone());

            let raw_extended = build_extended_fields(&event.extended_event_descriptors);

            let (genre1, sub_genre1) = event
                .content_descriptors
                .first()
                .and_then(|cd| cd.contents.first())
                .map_or((None, None), |c| {
                    (c.content_nibble_level_1, c.content_nibble_level_2)
                });

            let (video_stream_content, video_component_type) = event
                .component_descriptors
                .first()
                .map_or((None, None), |cd| {
                    let sc = cd.stream_content.as_deref().and_then(parse_hex_u8);
                    let ct = cd.component_type.as_deref().and_then(parse_hex_u8);
                    (sc, ct)
                });

            let (audio_component_type, audio_sampling_rate_code) = event
                .audio_component_descriptors
                .first()
                .map_or((None, None), |ad| {
                    let ct = ad.component_type.as_deref().and_then(parse_hex_u8);
                    (ct, ad.sampling_rate)
                });

            programs.push(ProgramInfo {
                service_id: sid,
                event_id: event.event_id,
                start_time: event.start_time,
                duration_sec,
                duration_raw: event.duration,
                running_status: event
                    .running_status
                    .unwrap_or_else(|| String::from("undefined")),
                program_name,
                description,
                table_type: table_type.clone(),
                raw_extended,
                genre1,
                sub_genre1,
                video_stream_content,
                video_component_type,
                audio_component_type,
                audio_sampling_rate_code,
            });
        }
    }
    Ok(programs)
}

/// Parse EIT events filtered by target service ID.
///
/// # Errors
///
/// Returns an error if the XML is malformed or service IDs cannot be parsed.
pub fn parse_eit_xml_by_sid(xml: &str, target_sid: &str) -> Result<Vec<ProgramInfo>> {
    let target = parse_sid(target_sid)
        .with_context(|| format!("invalid target service_id: {target_sid}"))?;
    let all = parse_eit_xml(xml).context("failed to parse EIT XML for SID filtering")?;
    Ok(all.into_iter().filter(|p| p.service_id == target).collect())
}

/// Load and parse EIT XML from a file.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the XML is malformed.
pub fn load(path: &Path) -> Result<Vec<ProgramInfo>> {
    let xml =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    parse_eit_xml(&xml)
}

/// Load and parse EIT XML from a file, filtered by service ID.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the XML is malformed.
pub fn load_by_sid(path: &Path, target_sid: &str) -> Result<Vec<ProgramInfo>> {
    let xml =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    parse_eit_xml_by_sid(&xml, target_sid)
}

/// Parse `HH:MM:SS` duration string to total seconds.
///
/// # Errors
///
/// Returns an error if the format is invalid.
pub fn parse_duration_to_sec(duration: &str) -> Result<u32> {
    let parts: Vec<&str> = duration.split(':').collect();
    if parts.len() != 3 {
        bail!("invalid duration format: {duration}");
    }
    let hours: u32 = parts
        .first()
        .context("missing hours in duration")?
        .parse()
        .with_context(|| format!("invalid hours in duration: {duration}"))?;
    let minutes: u32 = parts
        .get(1)
        .context("missing minutes in duration")?
        .parse()
        .with_context(|| format!("invalid minutes in duration: {duration}"))?;
    let seconds: u32 = parts
        .get(2)
        .context("missing seconds in duration")?
        .parse()
        .with_context(|| format!("invalid seconds in duration: {duration}"))?;
    hours
        .checked_mul(3600)
        .and_then(|h| minutes.checked_mul(60).and_then(|m| h.checked_add(m)))
        .and_then(|hm| hm.checked_add(seconds))
        .with_context(|| format!("duration overflow: {duration}"))
}

/// Parse `HH:MM:SS` duration string to total minutes (seconds truncated).
///
/// # Errors
///
/// Returns an error if the format is invalid.
pub fn parse_duration_to_min(duration: &str) -> Result<u32> {
    parse_duration_to_sec(duration).map(|s| s / 60)
}

/// Deduplicate programs by `(service_id, event_id)` pair, keeping the first occurrence.
#[must_use]
pub fn dedup_programs(programs: Vec<ProgramInfo>) -> Vec<ProgramInfo> {
    let mut seen = std::collections::HashSet::new();
    programs
        .into_iter()
        .filter(|p| seen.insert((p.service_id, p.event_id.clone())))
        .collect()
}

/// Detected recording target with the method used for detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingTarget {
    /// The program identified as the recording target.
    pub program: ProgramInfo,
    /// How the recording target was detected.
    pub detection_method: DetectionMethod,
}

/// How the recording target was detected from EIT data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionMethod {
    /// Detected via `running_status == "running"` in a p/f table.
    RunningStatus,
    /// First event in a p/f table (no running status found).
    FirstPfEvent,
    /// First event overall (no p/f tables found, fallback).
    FirstEvent,
}

/// Detect the recording target from middle-of-file EIT programs.
///
/// Detection priority:
/// 1. `table_type == "pf"` and `running_status == "running"` → [`DetectionMethod::RunningStatus`]
/// 2. First event in a `table_type == "pf"` table → [`DetectionMethod::FirstPfEvent`]
/// 3. First event overall → [`DetectionMethod::FirstEvent`]
#[must_use]
pub fn detect_recording_target(programs: &[ProgramInfo]) -> Option<RecordingTarget> {
    // Priority 1: running status in p/f table.
    if let Some(p) = programs
        .iter()
        .find(|p| p.table_type.as_deref() == Some("pf") && p.running_status == "running")
    {
        return Some(RecordingTarget {
            program: p.clone(),
            detection_method: DetectionMethod::RunningStatus,
        });
    }

    // Priority 2: first event in any p/f table.
    if let Some(p) = programs
        .iter()
        .find(|p| p.table_type.as_deref() == Some("pf"))
    {
        return Some(RecordingTarget {
            program: p.clone(),
            detection_method: DetectionMethod::FirstPfEvent,
        });
    }

    // Priority 3: first event overall.
    programs.first().map(|p| RecordingTarget {
        program: p.clone(),
        detection_method: DetectionMethod::FirstEvent,
    })
}

/// Strip hex prefix (`0x` / `0X`) from a string, returning the remainder.
///
/// Returns `None` if no prefix is found (i.e. the string is decimal).
fn strip_hex_prefix(s: &str) -> Option<&str> {
    s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
}

/// Parse a `u8` from decimal or hex (`0x..`) string.
fn parse_hex_u8(s: &str) -> Option<u8> {
    let trimmed = s.trim();
    strip_hex_prefix(trimmed).map_or_else(
        || trimmed.parse::<u8>().ok(),
        |hex| u8::from_str_radix(hex, 16).ok(),
    )
}

/// Decode ARIB STD-B10 audio sampling rate code to Hz.
#[must_use]
pub const fn decode_sampling_rate(code: u8) -> Option<u32> {
    match code {
        1 => Some(16000),
        2 => Some(22050),
        3 => Some(24000),
        5 => Some(32000),
        6 => Some(44100),
        7 => Some(48000),
        _ => None,
    }
}

/// Decode ARIB STD-B10 component type to video resolution string.
///
/// Uses the upper 4 bits of `component_type` to determine resolution.
#[must_use]
pub const fn decode_video_resolution(component_type: u8) -> Option<&'static str> {
    match component_type >> 4 {
        0x0 => Some("480i"),
        0x9 => Some("2160p"),
        0xA => Some("480p"),
        0xB => Some("1080i"),
        0xC => Some("720p"),
        0xD => Some("240p"),
        0xE => Some("1080p"),
        _ => None,
    }
}

/// Decode ARIB STD-B10 major genre code (content nibble level 1) to English name.
#[must_use]
pub const fn decode_genre(nibble1: u8) -> Option<&'static str> {
    match nibble1 {
        0x0 => Some("News/Report"),
        0x1 => Some("Sports"),
        0x2 => Some("Information/Tabloid"),
        0x3 => Some("Drama"),
        0x4 => Some("Music"),
        0x5 => Some("Variety"),
        0x6 => Some("Movie"),
        0x7 => Some("Animation/Special Effects"),
        0x8 => Some("Documentary/Education"),
        0x9 => Some("Theater/Performance"),
        0xA => Some("Hobby/Education"),
        0xB => Some("Welfare"),
        0xC | 0xD => Some("Reserved"),
        0xE => Some("Extended"),
        0xF => Some("Other"),
        _ => None,
    }
}

/// Build ordered key-value pairs from extended event descriptors.
///
/// When an item has an empty `description`, its value is appended to the
/// previous item (EIT continuation semantics).
#[must_use]
pub fn build_extended_fields(descriptors: &[ExtendedEventDescriptor]) -> Vec<(String, String)> {
    let mut raw: Vec<(String, String)> = Vec::new();

    // Sort by descriptor_number for correct ordering.
    let mut sorted: Vec<&ExtendedEventDescriptor> = descriptors.iter().collect();
    sorted.sort_by_key(|d| d.descriptor_number.unwrap_or(0));

    for desc in &sorted {
        for item in &desc.items {
            let key = item.description.as_deref().unwrap_or("");
            let value = item.name.as_deref().unwrap_or("");
            if key.is_empty() && value.is_empty() {
                continue;
            }
            if key.is_empty() {
                // Empty key = continuation of previous item.
                if let Some(last) = raw.last_mut() {
                    last.1.push_str(value);
                }
            } else if let Some(existing) = raw.iter_mut().find(|(k, _)| k == key) {
                // Same key seen before: append value.
                existing.1.push_str(value);
            } else {
                raw.push((key.to_owned(), value.to_owned()));
            }
        }
    }

    raw
}

/// Parse a service ID string, supporting both decimal and hex (`0x...`) formats.
pub(crate) fn parse_sid(sid_str: &str) -> Result<u32> {
    let trimmed = sid_str.trim();
    strip_hex_prefix(trimmed).map_or_else(
        || {
            trimmed
                .parse::<u32>()
                .with_context(|| format!("invalid decimal service_id: {trimmed}"))
        },
        |hex| {
            u32::from_str_radix(hex, 16)
                .with_context(|| format!("invalid hex service_id: {trimmed}"))
        },
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    /// Lookup value by key in `raw_extended` pairs.
    fn find_extended<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
        pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    // ── parse_duration_to_min ──

    #[test]
    fn test_parse_duration_30min() {
        assert_eq!(parse_duration_to_min("00:30:00").unwrap(), 30);
    }

    #[test]
    fn test_parse_duration_1h() {
        assert_eq!(parse_duration_to_min("01:00:00").unwrap(), 60);
    }

    #[test]
    fn test_parse_duration_1h30m() {
        assert_eq!(parse_duration_to_min("01:30:00").unwrap(), 90);
    }

    #[test]
    fn test_parse_duration_6min() {
        assert_eq!(parse_duration_to_min("00:06:00").unwrap(), 6);
    }

    #[test]
    fn test_parse_duration_zero() {
        assert_eq!(parse_duration_to_min("00:00:00").unwrap(), 0);
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration_to_min("invalid").is_err());
    }

    #[test]
    fn test_parse_duration_too_few_parts() {
        assert!(parse_duration_to_min("01:30").is_err());
    }

    // ── parse_sid ──

    #[test]
    fn test_parse_sid_decimal() {
        assert_eq!(parse_sid("65024").unwrap(), 65024);
    }

    #[test]
    fn test_parse_sid_hex_lowercase() {
        // 0xFE00 = 65024
        assert_eq!(parse_sid("0xFE00").unwrap(), 65024);
    }

    #[test]
    fn test_parse_sid_hex_uppercase_prefix() {
        assert_eq!(parse_sid("0XFE00").unwrap(), 65024);
    }

    #[test]
    fn test_parse_sid_small_decimal() {
        assert_eq!(parse_sid("1024").unwrap(), 1024);
    }

    #[test]
    fn test_parse_sid_invalid() {
        assert!(parse_sid("not_a_number").is_err());
    }

    #[test]
    fn test_parse_sid_invalid_hex() {
        assert!(parse_sid("0xZZZZ").is_err());
    }

    // ── parse_eit_xml ──

    #[test]
    fn test_parse_eit_xml_basic() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT version="1" actual="true" current="true"
       service_id="65024"
       transport_stream_id="10153"
       original_network_id="12345"
       last_table_id="78" type="pf">
    <event event_id="1001" start_time="2024-12-31 15:00:00"
           duration="00:06:00" running_status="running" CA_mode="false">
      <short_event_descriptor language_code="jpn">
        <event_name>JTV Gen Program</event_name>
        <text>ARIB compliant test stream</text>
      </short_event_descriptor>
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml(xml).unwrap();

        // Assert
        assert_eq!(programs.len(), 1);
        let p = &programs[0];
        assert_eq!(p.service_id, 65024);
        assert_eq!(p.event_id, "1001");
        assert_eq!(p.start_time, "2024-12-31 15:00:00");
        assert_eq!(p.duration_min(), 6);
        assert_eq!(p.duration_raw, "00:06:00");
        assert_eq!(p.running_status, "running");
        assert_eq!(p.program_name.as_deref(), Some("JTV Gen Program"));
        assert_eq!(p.table_type.as_deref(), Some("pf"));
    }

    #[test]
    fn test_parse_eit_xml_hex_service_id() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="0xFE00">
    <event event_id="2001" start_time="2025-01-01 00:00:00"
           duration="00:30:00" running_status="not-running">
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml(xml).unwrap();

        // Assert
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].service_id, 65024);
        assert_eq!(programs[0].duration_min(), 30);
        assert!(programs[0].program_name.is_none());
    }

    #[test]
    fn test_parse_eit_xml_multiple_events() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="1024">
    <event event_id="100" start_time="2025-01-01 08:00:00"
           duration="00:30:00" running_status="running">
      <short_event_descriptor language_code="jpn">
        <event_name>Morning News</event_name>
      </short_event_descriptor>
    </event>
    <event event_id="101" start_time="2025-01-01 08:30:00"
           duration="01:00:00" running_status="not-running">
      <short_event_descriptor language_code="jpn">
        <event_name>Drama</event_name>
      </short_event_descriptor>
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml(xml).unwrap();

        // Assert
        assert_eq!(programs.len(), 2);
        assert_eq!(programs[0].event_id, "100");
        assert_eq!(programs[0].program_name.as_deref(), Some("Morning News"));
        assert_eq!(programs[1].event_id, "101");
        assert_eq!(programs[1].duration_min(), 60);
    }

    #[test]
    fn test_parse_eit_xml_empty() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml(xml).unwrap();

        // Assert
        assert!(programs.is_empty());
    }

    #[test]
    fn test_parse_eit_xml_no_running_status() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="1024">
    <event event_id="100" start_time="2025-01-01 08:00:00"
           duration="00:30:00">
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml(xml).unwrap();

        // Assert
        assert_eq!(programs[0].running_status, "undefined");
    }

    // ── parse_eit_xml_by_sid ──

    #[test]
    fn test_parse_eit_xml_by_sid_filter() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="1024">
    <event event_id="100" start_time="2025-01-01 08:00:00"
           duration="00:30:00" running_status="running">
    </event>
  </EIT>
  <EIT service_id="2048">
    <event event_id="200" start_time="2025-01-01 09:00:00"
           duration="01:00:00" running_status="running">
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml_by_sid(xml, "1024").unwrap();

        // Assert
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].event_id, "100");
    }

    #[test]
    fn test_parse_eit_xml_by_sid_no_match() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="1024">
    <event event_id="100" start_time="2025-01-01 08:00:00"
           duration="00:30:00" running_status="running">
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml_by_sid(xml, "9999").unwrap();

        // Assert
        assert!(programs.is_empty());
    }

    #[test]
    fn test_parse_eit_xml_by_sid_hex_target() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="65024">
    <event event_id="100" start_time="2025-01-01 08:00:00"
           duration="00:30:00" running_status="running">
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml_by_sid(xml, "0xFE00").unwrap();

        // Assert
        assert_eq!(programs.len(), 1);
    }

    // ── parse_eit_xml (program_text / table_type) ──

    #[test]
    fn test_parse_eit_xml_with_text() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="1024" type="pf">
    <event event_id="500" start_time="2025-06-01 20:00:00"
           duration="00:30:00" running_status="running">
      <short_event_descriptor language_code="jpn">
        <event_name>Sample Show</event_name>
        <text>Episode 1: Pilot</text>
      </short_event_descriptor>
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml(xml).unwrap();

        // Assert
        assert_eq!(programs.len(), 1);
        let p = &programs[0];
        assert_eq!(p.program_name.as_deref(), Some("Sample Show"));
        assert_eq!(p.table_type.as_deref(), Some("pf"));
    }

    #[test]
    fn test_parse_eit_xml_schedule_type() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="2048" type="schedule">
    <event event_id="600" start_time="2025-06-02 10:00:00"
           duration="01:00:00" running_status="not-running">
      <short_event_descriptor language_code="jpn">
        <event_name>Scheduled Program</event_name>
      </short_event_descriptor>
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml(xml).unwrap();

        // Assert
        assert_eq!(programs.len(), 1);
        let p = &programs[0];
        assert_eq!(p.table_type.as_deref(), Some("schedule"));
    }

    // ── load / load_by_sid ──

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_from_file() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eit.xml");
        std::fs::write(
            &path,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="1024">
    <event event_id="100" start_time="2025-01-01 08:00:00"
           duration="00:30:00" running_status="running">
      <short_event_descriptor language_code="jpn">
        <event_name>Test Program</event_name>
      </short_event_descriptor>
    </event>
  </EIT>
</tsduck>"#,
        )
        .unwrap();

        // Act
        let programs = load(&path).unwrap();

        // Assert
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].program_name.as_deref(), Some("Test Program"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_file_not_found() {
        // Act
        let result = load(Path::new("/nonexistent/eit.xml"));

        // Assert
        assert!(result.is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_load_by_sid_from_file() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eit.xml");
        std::fs::write(
            &path,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="1024">
    <event event_id="100" start_time="2025-01-01 08:00:00"
           duration="00:30:00" running_status="running">
    </event>
  </EIT>
  <EIT service_id="2048">
    <event event_id="200" start_time="2025-01-01 09:00:00"
           duration="01:00:00" running_status="running">
    </event>
  </EIT>
</tsduck>"#,
        )
        .unwrap();

        // Act
        let programs = load_by_sid(&path, "2048").unwrap();

        // Assert
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].event_id, "200");
    }

    // ── dedup_programs ──

    fn make_program(sid: u32, event_id: &str, name: Option<&str>) -> ProgramInfo {
        make_program_full(sid, event_id, name, "undefined", Some("pf"))
    }

    #[test]
    fn test_dedup_programs_removes_duplicates() {
        // Arrange
        let programs = vec![
            make_program(1024, "100", Some("First")),
            make_program(1024, "100", Some("Duplicate")),
            make_program(1024, "101", Some("Second")),
        ];

        // Act
        let result = dedup_programs(programs);

        // Assert
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].program_name.as_deref(), Some("First"));
        assert_eq!(result[1].event_id, "101");
    }

    #[test]
    fn test_dedup_programs_different_sids_kept() {
        // Arrange
        let programs = vec![
            make_program(1024, "100", Some("SID-A")),
            make_program(2048, "100", Some("SID-B")),
        ];

        // Act
        let result = dedup_programs(programs);

        // Assert
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_dedup_programs_empty() {
        // Act
        let result = dedup_programs(vec![]);

        // Assert
        assert!(result.is_empty());
    }

    // ── detect_recording_target ──

    fn make_program_full(
        sid: u32,
        event_id: &str,
        name: Option<&str>,
        running_status: &str,
        table_type: Option<&str>,
    ) -> ProgramInfo {
        ProgramInfo {
            service_id: sid,
            event_id: event_id.to_owned(),
            start_time: "2025-01-01 00:00:00".to_owned(),
            duration_sec: 1800,
            duration_raw: "00:30:00".to_owned(),
            running_status: running_status.to_owned(),
            program_name: name.map(ToOwned::to_owned),
            description: None,
            table_type: table_type.map(ToOwned::to_owned),
            raw_extended: Vec::new(),
            genre1: None,
            sub_genre1: None,
            video_stream_content: None,
            video_component_type: None,
            audio_component_type: None,
            audio_sampling_rate_code: None,
        }
    }

    #[test]
    fn test_detect_target_running_status() {
        // Arrange — p/f table with one running, one not-running
        let programs = vec![
            make_program_full(1024, "100", Some("Prev"), "not-running", Some("pf")),
            make_program_full(1024, "101", Some("Current"), "running", Some("pf")),
        ];

        // Act
        let target = detect_recording_target(&programs);

        // Assert
        let target = target.unwrap();
        assert_eq!(target.program.event_id, "101");
        assert_eq!(target.program.program_name.as_deref(), Some("Current"));
        assert_eq!(target.detection_method, DetectionMethod::RunningStatus);
    }

    #[test]
    fn test_detect_target_first_pf_event() {
        // Arrange — p/f table but no running status
        let programs = vec![
            make_program_full(1024, "100", Some("First"), "undefined", Some("pf")),
            make_program_full(1024, "101", Some("Second"), "undefined", Some("pf")),
        ];

        // Act
        let target = detect_recording_target(&programs);

        // Assert
        let target = target.unwrap();
        assert_eq!(target.program.event_id, "100");
        assert_eq!(target.detection_method, DetectionMethod::FirstPfEvent);
    }

    #[test]
    fn test_detect_target_first_event_fallback() {
        // Arrange — no p/f tables, only schedule
        let programs = vec![
            make_program_full(1024, "200", Some("Sched1"), "not-running", Some("schedule")),
            make_program_full(1024, "201", Some("Sched2"), "not-running", Some("schedule")),
        ];

        // Act
        let target = detect_recording_target(&programs);

        // Assert
        let target = target.unwrap();
        assert_eq!(target.program.event_id, "200");
        assert_eq!(target.detection_method, DetectionMethod::FirstEvent);
    }

    #[test]
    fn test_detect_target_empty() {
        // Act
        let target = detect_recording_target(&[]);

        // Assert
        assert!(target.is_none());
    }

    #[test]
    fn test_detect_target_running_over_not_running_pf() {
        // Arrange — running in second p/f event takes priority
        let programs = vec![
            make_program_full(1024, "100", Some("NotRunning"), "not-running", Some("pf")),
            make_program_full(1024, "101", Some("Running"), "running", Some("pf")),
            make_program_full(1024, "200", Some("Schedule"), "running", Some("schedule")),
        ];

        // Act
        let target = detect_recording_target(&programs);

        // Assert
        let target = target.unwrap();
        assert_eq!(target.program.event_id, "101");
        assert_eq!(target.detection_method, DetectionMethod::RunningStatus);
    }

    #[test]
    fn test_detect_target_no_table_type() {
        // Arrange — no table_type set at all
        let programs = vec![make_program_full(
            1024,
            "100",
            Some("NoType"),
            "running",
            None,
        )];

        // Act
        let target = detect_recording_target(&programs);

        // Assert
        let target = target.unwrap();
        assert_eq!(target.program.event_id, "100");
        assert_eq!(target.detection_method, DetectionMethod::FirstEvent);
    }

    // ── parse_duration_to_sec ──

    #[test]
    fn test_parse_duration_to_sec_30min() {
        assert_eq!(parse_duration_to_sec("00:30:00").unwrap(), 1800);
    }

    #[test]
    fn test_parse_duration_to_sec_1h() {
        assert_eq!(parse_duration_to_sec("01:00:00").unwrap(), 3600);
    }

    #[test]
    fn test_parse_duration_to_sec_30sec() {
        assert_eq!(parse_duration_to_sec("00:00:30").unwrap(), 30);
    }

    #[test]
    fn test_parse_duration_to_sec_mixed() {
        assert_eq!(parse_duration_to_sec("01:30:45").unwrap(), 5445);
    }

    // ── decode_sampling_rate ──

    #[test]
    fn test_decode_sampling_rate_48000() {
        assert_eq!(decode_sampling_rate(7), Some(48000));
    }

    #[test]
    fn test_decode_sampling_rate_32000() {
        assert_eq!(decode_sampling_rate(5), Some(32000));
    }

    #[test]
    fn test_decode_sampling_rate_unknown() {
        assert_eq!(decode_sampling_rate(0), None);
    }

    // ── decode_video_resolution ──

    #[test]
    fn test_decode_video_resolution_1080i() {
        // 179 = 0xB3, upper nibble 0xB → "1080i"
        assert_eq!(decode_video_resolution(0xB3), Some("1080i"));
    }

    #[test]
    fn test_decode_video_resolution_720p() {
        assert_eq!(decode_video_resolution(0xC3), Some("720p"));
    }

    #[test]
    fn test_decode_video_resolution_unknown() {
        assert_eq!(decode_video_resolution(0xF0), None);
    }

    // ── build_extended_fields ──

    #[test]
    fn test_build_extended_fields_empty() {
        // Act
        let raw = build_extended_fields(&[]);

        // Assert
        assert!(raw.is_empty());
    }

    #[test]
    fn test_build_extended_fields_duplicate_keys() {
        // Arrange — same key across two descriptors should concatenate values.
        let descriptors = vec![
            ExtendedEventDescriptor {
                descriptor_number: Some(0),
                last_descriptor_number: Some(1),
                language_code: Some("jpn".to_owned()),
                items: vec![ExtendedEventItem {
                    description: Some("出演者".to_owned()),
                    name: Some("田中太郎".to_owned()),
                }],
                text: None,
            },
            ExtendedEventDescriptor {
                descriptor_number: Some(1),
                last_descriptor_number: Some(1),
                language_code: Some("jpn".to_owned()),
                items: vec![ExtendedEventItem {
                    description: Some("出演者".to_owned()),
                    name: Some("山田花子".to_owned()),
                }],
                text: None,
            },
        ];

        // Act
        let raw = build_extended_fields(&descriptors);

        // Assert
        assert_eq!(find_extended(&raw, "出演者"), Some("田中太郎山田花子"));
    }

    #[test]
    fn test_build_extended_fields_diamond_prefix_preserved() {
        // Arrange — key already starts with ◇ should be stored as-is.
        let descriptors = vec![ExtendedEventDescriptor {
            descriptor_number: Some(0),
            last_descriptor_number: Some(0),
            language_code: Some("jpn".to_owned()),
            items: vec![ExtendedEventItem {
                description: Some("◇あらすじ◇".to_owned()),
                name: Some("物語の概要".to_owned()),
            }],
            text: None,
        }];

        // Act
        let raw = build_extended_fields(&descriptors);

        // Assert
        assert_eq!(raw[0].0, "◇あらすじ◇");
        assert_eq!(raw[0].1, "物語の概要");
    }

    #[test]
    fn test_build_extended_fields_empty_key_continuation() {
        // Arrange — empty description means continuation of previous item.
        let descriptors = vec![
            ExtendedEventDescriptor {
                descriptor_number: Some(0),
                last_descriptor_number: Some(1),
                language_code: Some("jpn".to_owned()),
                items: vec![ExtendedEventItem {
                    description: Some("あらすじ".to_owned()),
                    name: Some("前半テキスト".to_owned()),
                }],
                text: None,
            },
            ExtendedEventDescriptor {
                descriptor_number: Some(1),
                last_descriptor_number: Some(1),
                language_code: Some("jpn".to_owned()),
                items: vec![ExtendedEventItem {
                    description: Some(String::new()),
                    name: Some("後半テキスト".to_owned()),
                }],
                text: None,
            },
        ];

        // Act
        let raw = build_extended_fields(&descriptors);

        // Assert — empty key continuation is merged into previous entry.
        assert_eq!(raw.len(), 1);
        assert_eq!(
            find_extended(&raw, "あらすじ"),
            Some("前半テキスト後半テキスト")
        );
    }

    #[test]
    fn test_build_extended_fields_preserves_insertion_order() {
        // Arrange — items should appear in EIT order, not alphabetical.
        let descriptors = vec![ExtendedEventDescriptor {
            descriptor_number: Some(0),
            last_descriptor_number: Some(0),
            language_code: Some("jpn".to_owned()),
            items: vec![
                ExtendedEventItem {
                    description: Some("出演者".to_owned()),
                    name: Some("沢城みゆき".to_owned()),
                },
                ExtendedEventItem {
                    description: Some("あらすじ".to_owned()),
                    name: Some("物語の概要".to_owned()),
                },
            ],
            text: None,
        }];

        // Act
        let raw = build_extended_fields(&descriptors);

        // Assert — 出演者 appears first (insertion order), not あらすじ (alphabetical).
        assert_eq!(raw[0].0, "出演者");
        assert_eq!(raw[1].0, "あらすじ");
    }

    // ── parse_eit_xml full descriptors ──

    #[test]
    fn test_parse_eit_xml_full_descriptors() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="1024" type="pf">
    <event event_id="500" start_time="2025-06-01 20:00:00"
           duration="01:30:45" running_status="running">
      <short_event_descriptor language_code="jpn">
        <event_name>Test Show</event_name>
        <text>Episode 1</text>
      </short_event_descriptor>
      <extended_event_descriptor descriptor_number="0" last_descriptor_number="0" language_code="jpn">
        <item>
          <description>出演者</description>
          <name>沢城みゆき</name>
        </item>
      </extended_event_descriptor>
      <content_descriptor>
        <content content_nibble_level_1="7" content_nibble_level_2="3"/>
      </content_descriptor>
      <component_descriptor stream_content="1" component_type="179" language_code="jpn">
        <text>1080i</text>
      </component_descriptor>
      <audio_component_descriptor stream_content="2" component_type="3" sampling_rate="7" language_code="jpn"/>
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml(xml).unwrap();

        // Assert
        assert_eq!(programs.len(), 1);
        let p = &programs[0];
        assert_eq!(p.program_name.as_deref(), Some("Test Show"));
        assert_eq!(p.description.as_deref(), Some("Episode 1"));
        assert_eq!(p.duration_sec, 5445);
        assert_eq!(p.duration_min(), 90);
        assert!(p.extended().unwrap().contains("出演者"));
        assert_eq!(find_extended(&p.raw_extended, "出演者"), Some("沢城みゆき"));
        assert_eq!(p.genre1, Some(7));
        assert_eq!(p.sub_genre1, Some(3));
        assert_eq!(p.video_stream_content, Some(1));
        assert_eq!(p.video_component_type, Some(179));
        assert_eq!(p.video_resolution(), Some("1080i"));
        assert_eq!(p.audio_component_type, Some(3));
        assert_eq!(p.audio_sampling_rate(), Some(48000));
    }

    #[test]
    fn test_parse_eit_xml_multiple_extended() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="1024" type="pf">
    <event event_id="600" start_time="2025-06-01 20:00:00"
           duration="00:30:00" running_status="running">
      <extended_event_descriptor descriptor_number="0" last_descriptor_number="1" language_code="jpn">
        <item>
          <description>出演者</description>
          <name>田中</name>
        </item>
      </extended_event_descriptor>
      <extended_event_descriptor descriptor_number="1" last_descriptor_number="1" language_code="jpn">
        <item>
          <description>出演者</description>
          <name>山田</name>
        </item>
        <item>
          <description>あらすじ</description>
          <name>物語</name>
        </item>
      </extended_event_descriptor>
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml(xml).unwrap();

        // Assert
        let p = &programs[0];
        assert_eq!(find_extended(&p.raw_extended, "出演者"), Some("田中山田"));
        assert_eq!(find_extended(&p.raw_extended, "あらすじ"), Some("物語"));
        let ext = p.extended().unwrap();
        assert!(ext.contains("◇あらすじ\n物語"));
        assert!(ext.contains("◇出演者\n田中山田"));
    }

    #[test]
    fn test_parse_eit_xml_no_optional_descriptors() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="1024">
    <event event_id="700" start_time="2025-06-01 20:00:00"
           duration="00:30:00">
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml(xml).unwrap();

        // Assert
        let p = &programs[0];
        assert!(p.description.is_none());
        assert!(p.extended().is_none());
        assert!(p.raw_extended.is_empty());
        assert!(p.genre1.is_none());
        assert!(p.sub_genre1.is_none());
        assert!(p.video_stream_content.is_none());
        assert!(p.video_component_type.is_none());
        assert!(p.video_resolution().is_none());
        assert!(p.audio_component_type.is_none());
        assert!(p.audio_sampling_rate().is_none());
    }

    #[test]
    fn test_parse_eit_xml_hex_attribute_values() {
        // Arrange — TSDuck outputs hex strings like "0x01", "0xB3".
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="0x5C39" type="pf">
    <event event_id="0xF5B6" start_time="2026-03-02 00:00:00"
           duration="00:30:00" running_status="undefined">
      <short_event_descriptor language_code="jpn">
        <event_name>Test</event_name>
        <text>Desc</text>
      </short_event_descriptor>
      <component_descriptor stream_content="0x01" component_type="0xB3" language_code="jpn">
        <text>1080i</text>
      </component_descriptor>
      <content_descriptor>
        <content content_nibble_level_1="7" content_nibble_level_2="0"/>
      </content_descriptor>
      <audio_component_descriptor stream_content="0x02" component_type="0x03" sampling_rate="7" ISO_639_language_code="jpn"/>
    </event>
  </EIT>
</tsduck>"#;

        // Act
        let programs = parse_eit_xml(xml).unwrap();

        // Assert
        assert_eq!(programs.len(), 1);
        let p = &programs[0];
        assert_eq!(p.service_id, 0x5C39);
        assert_eq!(p.video_stream_content, Some(0x01));
        assert_eq!(p.video_component_type, Some(0xB3));
        assert_eq!(p.video_resolution(), Some("1080i"));
        assert_eq!(p.genre1, Some(7));
        assert_eq!(p.sub_genre1, Some(0));
        assert_eq!(p.audio_component_type, Some(0x03));
        assert_eq!(p.audio_sampling_rate(), Some(48000));
    }
}
