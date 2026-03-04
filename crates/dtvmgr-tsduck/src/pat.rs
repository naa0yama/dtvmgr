//! PAT (Program Association Table) XML parser.
//!
//! Extracts service IDs from `TSDuck` `tstables` XML output.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::eit::parse_sid;

/// Root element of `TSDuck` XML output containing PAT tables.
#[derive(Debug, Deserialize)]
struct TsduckPatXml {
    /// PAT tables in the XML.
    #[serde(rename = "PAT", default)]
    pat_tables: Vec<PatTable>,
}

/// A single PAT table.
#[derive(Debug, Deserialize)]
struct PatTable {
    /// Services listed in this PAT.
    #[serde(rename = "service", default)]
    services: Vec<PatService>,
}

/// A service entry within a PAT table.
#[derive(Debug, Deserialize)]
struct PatService {
    /// Service ID (decimal or hex like `0x5C38`).
    #[serde(rename = "@service_id")]
    service_id: String,
}

/// Extract the first service ID from PAT in `TSDuck` XML.
///
/// Returns the first `service_id` found across all PAT tables, or `None` if
/// no PAT table or service entry exists in the XML.
///
/// # Errors
///
/// Returns an error if the XML is malformed or a service ID cannot be parsed.
pub fn parse_pat_first_service_id(xml: &str) -> Result<Option<u32>> {
    let doc: TsduckPatXml =
        quick_xml::de::from_str(xml).context("failed to parse TSDuck PAT XML")?;

    for table in &doc.pat_tables {
        if let Some(svc) = table.services.first() {
            let sid = parse_sid(&svc.service_id)
                .with_context(|| format!("invalid PAT service_id: {}", svc.service_id))?;
            return Ok(Some(sid));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn test_parse_pat_single_service() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <PAT transport_stream_id="0x7FE9" version="3">
    <service service_id="0x5C38" program_map_PID="0x0101"/>
  </PAT>
</tsduck>"#;

        // Act
        let sid = parse_pat_first_service_id(xml).unwrap();

        // Assert
        assert_eq!(sid, Some(0x5C38));
    }

    #[test]
    fn test_parse_pat_multiple_services() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <PAT transport_stream_id="0x7FE9" version="3">
    <service service_id="0x5C38" program_map_PID="0x0101"/>
    <service service_id="0x5C39" program_map_PID="0x0102"/>
  </PAT>
</tsduck>"#;

        // Act
        let sid = parse_pat_first_service_id(xml).unwrap();

        // Assert — returns the first service
        assert_eq!(sid, Some(0x5C38));
    }

    #[test]
    fn test_parse_pat_no_pat() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <EIT service_id="1024">
    <event event_id="100" start_time="2025-01-01 08:00:00"
           duration="00:30:00"/>
  </EIT>
</tsduck>"#;

        // Act
        let sid = parse_pat_first_service_id(xml).unwrap();

        // Assert
        assert_eq!(sid, None);
    }

    #[test]
    fn test_parse_pat_hex_service_id() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <PAT>
    <service service_id="0xFE00"/>
  </PAT>
</tsduck>"#;

        // Act
        let sid = parse_pat_first_service_id(xml).unwrap();

        // Assert
        assert_eq!(sid, Some(65024));
    }

    #[test]
    fn test_parse_pat_decimal_service_id() {
        // Arrange
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <PAT>
    <service service_id="23608"/>
  </PAT>
</tsduck>"#;

        // Act
        let sid = parse_pat_first_service_id(xml).unwrap();

        // Assert
        assert_eq!(sid, Some(23608));
    }

    #[test]
    fn test_parse_pat_empty_pat() {
        // Arrange — PAT exists but has no services
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tsduck>
  <PAT transport_stream_id="0x7FE9" version="3">
  </PAT>
</tsduck>"#;

        // Act
        let sid = parse_pat_first_service_id(xml).unwrap();

        // Assert
        assert_eq!(sid, None);
    }
}
