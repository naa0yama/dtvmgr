//! XML response wrapper types and custom deserializers.

use serde::de::Error;
use serde::{Deserialize, Deserializer};

use super::types::{SyoboiChannel, SyoboiProgram, SyoboiTitle};

/// Deserializes empty strings as `None` (for `String` fields).
pub fn deserialize_empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let result = Option::deserialize(deserializer);
    let s: Option<String> = result.map_err(D::Error::custom)?;
    Ok(s.filter(|s| !s.is_empty()))
}

/// Deserializes empty strings as `None` (for `u32` fields).
pub fn deserialize_empty_string_as_none_u32<'de, D>(
    deserializer: D,
) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let result = Option::deserialize(deserializer);
    let s: Option<String> = result.map_err(D::Error::custom)?;
    match s.as_deref() {
        None | Some("") => Ok(None),
        Some(v) => v
            .parse::<u32>()
            .map(Some)
            .map_err(|e| D::Error::custom(format!("failed to parse u32: {e}"))),
    }
}

/// Deserializes empty strings as `None` (for `i32` fields).
pub fn deserialize_empty_string_as_none_i32<'de, D>(
    deserializer: D,
) -> Result<Option<i32>, D::Error>
where
    D: Deserializer<'de>,
{
    let result = Option::deserialize(deserializer);
    let s: Option<String> = result.map_err(D::Error::custom)?;
    match s.as_deref() {
        None | Some("") => Ok(None),
        Some(v) => v
            .parse::<i32>()
            .map(Some)
            .map_err(|e| D::Error::custom(format!("failed to parse i32: {e}"))),
    }
}

/// API result status.
#[derive(Debug, Deserialize)]
pub struct ApiResult {
    /// Status code.
    #[serde(rename = "Code")]
    pub code: u32,
    /// Optional message.
    #[serde(
        rename = "Message",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub message: Option<String>,
}

/// `TitleLookup` full response.
#[derive(Debug, Deserialize)]
#[serde(rename = "TitleLookupResponse")]
pub struct TitleLookupResponse {
    #[serde(rename = "Result")]
    pub result: ApiResult,
    #[serde(rename = "TitleItems")]
    pub title_items: TitleItems,
}

/// `TitleItems` container.
#[derive(Debug, Deserialize)]
pub struct TitleItems {
    #[serde(rename = "TitleItem", default)]
    pub items: Vec<SyoboiTitle>,
}

/// `ProgLookup` full response.
#[derive(Debug, Deserialize)]
#[serde(rename = "ProgLookupResponse")]
pub struct ProgLookupResponse {
    #[serde(rename = "ProgItems")]
    pub prog_items: ProgItems,
}

/// `ProgItems` container.
#[derive(Debug, Deserialize)]
pub struct ProgItems {
    #[serde(rename = "ProgItem", default)]
    pub items: Vec<SyoboiProgram>,
}

/// `ChLookup` full response.
#[derive(Debug, Deserialize)]
#[serde(rename = "ChLookupResponse")]
pub struct ChLookupResponse {
    #[serde(rename = "ChItems")]
    pub ch_items: ChItems,
}

/// `ChItems` container.
#[derive(Debug, Deserialize)]
pub struct ChItems {
    #[serde(rename = "ChItem", default)]
    pub items: Vec<SyoboiChannel>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn test_deserialize_empty_string_as_none() {
        // Arrange
        #[derive(Deserialize)]
        struct Test {
            #[serde(deserialize_with = "deserialize_empty_string_as_none", default)]
            value: Option<String>,
        }

        // Act & Assert
        let result: Test = quick_xml::de::from_str("<Test><value></value></Test>").unwrap();
        assert_eq!(result.value, None);

        let result: Test = quick_xml::de::from_str("<Test><value>hello</value></Test>").unwrap();
        assert_eq!(result.value.as_deref(), Some("hello"));
    }

    #[test]
    fn test_deserialize_empty_string_as_none_u32() {
        // Arrange
        #[derive(Deserialize)]
        struct Test {
            #[serde(deserialize_with = "deserialize_empty_string_as_none_u32", default)]
            value: Option<u32>,
        }

        // Act & Assert
        let result: Test = quick_xml::de::from_str("<Test><value></value></Test>").unwrap();
        assert_eq!(result.value, None);

        let result: Test = quick_xml::de::from_str("<Test><value>42</value></Test>").unwrap();
        assert_eq!(result.value, Some(42));
    }

    #[test]
    fn test_deserialize_empty_string_as_none_i32() {
        // Arrange
        #[derive(Deserialize)]
        struct Test {
            #[serde(deserialize_with = "deserialize_empty_string_as_none_i32", default)]
            value: Option<i32>,
        }

        // Act & Assert
        let result: Test = quick_xml::de::from_str("<Test><value></value></Test>").unwrap();
        assert_eq!(result.value, None);

        let result: Test = quick_xml::de::from_str("<Test><value>-5</value></Test>").unwrap();
        assert_eq!(result.value, Some(-5));
    }

    #[test]
    fn test_parse_title_lookup_response() {
        // Arrange
        let xml = include_str!("../../../fixtures/syoboi/title_lookup_6309.xml");

        // Act
        let response: TitleLookupResponse = quick_xml::de::from_str(xml).unwrap();

        // Assert
        assert_eq!(response.result.code, 200);
        assert_eq!(response.title_items.items.len(), 1);
        assert_eq!(response.title_items.items[0].tid, 6309);
    }

    #[test]
    fn test_parse_prog_lookup_response() {
        // Arrange
        let xml = include_str!("../../../fixtures/syoboi/prog_lookup_6309.xml");

        // Act
        let response: ProgLookupResponse = quick_xml::de::from_str(xml).unwrap();

        // Assert
        assert_eq!(response.prog_items.items.len(), 3);
        assert_eq!(response.prog_items.items[0].pid, 574_823);
    }

    #[test]
    fn test_parse_ch_lookup_response() {
        // Arrange
        let xml = include_str!("../../../fixtures/syoboi/ch_lookup_all.xml");

        // Act
        let response: ChLookupResponse = quick_xml::de::from_str(xml).unwrap();

        // Assert
        assert_eq!(response.ch_items.items.len(), 3);
        assert_eq!(response.ch_items.items[0].ch_name, "NHK総合");
    }

    #[test]
    fn test_parse_empty_response() {
        // Arrange
        let xml = include_str!("../../../fixtures/syoboi/empty_response.xml");

        // Act
        let response: TitleLookupResponse = quick_xml::de::from_str(xml).unwrap();

        // Assert
        assert_eq!(response.result.code, 200);
        assert!(response.title_items.items.is_empty());
    }
}
