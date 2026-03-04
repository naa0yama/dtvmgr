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

/// Parsed program information extracted from EIT data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramInfo {
    /// Service ID (numeric).
    pub service_id: u32,
    /// Event ID string.
    pub event_id: String,
    /// Start time string.
    pub start_time: String,
    /// Duration in minutes.
    pub duration_min: u32,
    /// Raw duration string (`HH:MM:SS`).
    pub duration_raw: String,
    /// Running status.
    pub running_status: String,
    /// Program name from short event descriptor.
    pub program_name: Option<String>,
    /// EIT table type (`"pf"` or `"schedule"`).
    pub table_type: Option<String>,
}

/// Parse all EIT events from `TSDuck` XML output.
///
/// # Errors
///
/// Returns an error if the XML is malformed or service IDs cannot be parsed.
pub fn parse_eit_xml(xml: &str) -> Result<Vec<ProgramInfo>> {
    let doc: TsduckXml = quick_xml::de::from_str(xml).context("failed to parse TSDuck EIT XML")?;

    let mut programs = Vec::new();
    for table in &doc.eit_tables {
        let sid = parse_sid(&table.service_id)
            .with_context(|| format!("invalid service_id: {}", table.service_id))?;
        for event in &table.events {
            let duration_min = parse_duration_to_min(&event.duration).unwrap_or(0);
            let program_name = event
                .short_event_descriptors
                .first()
                .and_then(|d| d.event_name.clone());
            programs.push(ProgramInfo {
                service_id: sid,
                event_id: event.event_id.clone(),
                start_time: event.start_time.clone(),
                duration_min,
                duration_raw: event.duration.clone(),
                running_status: event
                    .running_status
                    .clone()
                    .unwrap_or_else(|| String::from("undefined")),
                program_name,
                table_type: table.table_type.clone(),
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

/// Parse `HH:MM:SS` duration string to total minutes.
///
/// # Errors
///
/// Returns an error if the format is invalid.
pub fn parse_duration_to_min(duration: &str) -> Result<u32> {
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
    // Seconds are truncated (not rounded).
    hours
        .checked_mul(60)
        .and_then(|h| h.checked_add(minutes))
        .with_context(|| format!("duration overflow: {duration}"))
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

/// Parse a service ID string, supporting both decimal and hex (`0x...`) formats.
pub(crate) fn parse_sid(sid_str: &str) -> Result<u32> {
    let trimmed = sid_str.trim();
    trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .map_or_else(
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
        assert_eq!(p.duration_min, 6);
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
        assert_eq!(programs[0].duration_min, 30);
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
        assert_eq!(programs[1].duration_min, 60);
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
        ProgramInfo {
            service_id: sid,
            event_id: event_id.to_owned(),
            start_time: "2025-01-01 00:00:00".to_owned(),
            duration_min: 30,
            duration_raw: "00:30:00".to_owned(),
            running_status: "undefined".to_owned(),
            program_name: name.map(ToOwned::to_owned),
            table_type: Some("pf".to_owned()),
        }
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
}
