//! XML response wrapper types and custom deserializers.

use serde::de::Error;
use serde::{Deserialize, Deserializer};

use super::types::{SyoboiChannel, SyoboiChannelGroup, SyoboiProgram, SyoboiTitle};

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
    /// API result status (absent in some responses).
    #[serde(rename = "Result", default)]
    pub result: Option<ApiResult>,
    /// Title items (absent in error responses).
    #[serde(rename = "TitleItems", default)]
    pub title_items: Option<TitleItems>,
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
    /// API result status.
    #[serde(rename = "Result", default)]
    pub result: Option<ApiResult>,
    /// Program items (absent in error responses).
    #[serde(rename = "ProgItems", default)]
    pub prog_items: Option<ProgItems>,
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
    /// API result status.
    #[serde(rename = "Result", default)]
    pub result: Option<ApiResult>,
    /// Channel items (absent in error responses).
    #[serde(rename = "ChItems", default)]
    pub ch_items: Option<ChItems>,
}

/// `ChItems` container.
#[derive(Debug, Deserialize)]
pub struct ChItems {
    #[serde(rename = "ChItem", default)]
    pub items: Vec<SyoboiChannel>,
}

/// `ChGroupLookup` full response.
#[derive(Debug, Deserialize)]
#[serde(rename = "ChGroupLookupResponse")]
pub struct ChGroupLookupResponse {
    /// API result status.
    #[serde(rename = "Result", default)]
    pub result: Option<ApiResult>,
    /// Channel group items (absent in error responses).
    #[serde(rename = "ChGroupItems", default)]
    pub ch_group_items: Option<ChGroupItems>,
}

/// `ChGroupItems` container.
#[derive(Debug, Deserialize)]
pub struct ChGroupItems {
    #[serde(rename = "ChGroupItem", default)]
    pub items: Vec<SyoboiChannelGroup>,
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
        let xml = include_str!("../../../../fixtures/syoboi/title_lookup_6309.xml");

        // Act
        let response: TitleLookupResponse = quick_xml::de::from_str(xml).unwrap();

        // Assert
        assert_eq!(response.result.as_ref().unwrap().code, 200);
        let items = response.title_items.unwrap();
        assert_eq!(items.items.len(), 1);
        assert_eq!(items.items[0].tid, 6309);
    }

    #[test]
    fn test_parse_prog_lookup_response() {
        // Arrange
        let xml = include_str!("../../../../fixtures/syoboi/prog_lookup_6309.xml");

        // Act
        let response: ProgLookupResponse = quick_xml::de::from_str(xml).unwrap();

        // Assert
        assert_eq!(response.result.as_ref().unwrap().code, 200);
        let items = response.prog_items.unwrap();
        assert_eq!(items.items.len(), 3);
        assert_eq!(items.items[0].pid, 574_823);
    }

    #[test]
    fn test_parse_ch_lookup_response() {
        // Arrange
        let xml = include_str!("../../../../fixtures/syoboi/ch_lookup_all.xml");

        // Act
        let response: ChLookupResponse = quick_xml::de::from_str(xml).unwrap();

        // Assert
        assert_eq!(response.result.as_ref().unwrap().code, 200);
        let items = response.ch_items.unwrap();
        assert_eq!(items.items.len(), 3);
        assert_eq!(items.items[0].ch_name, "NHK総合");
    }

    #[test]
    fn test_parse_ch_group_lookup_response() {
        // Arrange
        let xml = include_str!("../../../../fixtures/syoboi/ch_group_lookup_all.xml");

        // Act
        let response: ChGroupLookupResponse = quick_xml::de::from_str(xml).unwrap();

        // Assert
        assert_eq!(response.result.as_ref().unwrap().code, 200);
        let items = response.ch_group_items.unwrap();
        assert_eq!(items.items.len(), 3);
        assert_eq!(items.items[0].ch_gid, 1);
        assert_eq!(items.items[0].ch_group_name, "テレビ 関東");
        assert_eq!(items.items[0].ch_group_order, 1200);
    }

    #[test]
    fn test_parse_empty_response() {
        // Arrange
        let xml = include_str!("../../../../fixtures/syoboi/empty_response.xml");

        // Act
        let response: TitleLookupResponse = quick_xml::de::from_str(xml).unwrap();

        // Assert
        assert_eq!(response.result.as_ref().unwrap().code, 200);
        assert!(response.title_items.unwrap().items.is_empty());
    }

    #[test]
    fn test_parse_title_response_without_title_items() {
        // Arrange: error response with Result but no TitleItems
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<TitleLookupResponse>
    <Result>
        <Code>200</Code>
        <Message></Message>
    </Result>
</TitleLookupResponse>"#;

        // Act
        let response: TitleLookupResponse = quick_xml::de::from_str(xml).unwrap();

        // Assert
        assert!(response.title_items.is_none());
    }

    #[test]
    fn test_parse_title_response_without_result_element() {
        // Arrange: some API responses omit the <Result> element entirely
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<TitleLookupResponse>
    <TitleItems>
        <TitleItem id="100">
            <TID>100</TID>
            <LastUpdate>2024-01-01 00:00:00</LastUpdate>
            <Title>Test Title</Title>
            <ShortTitle></ShortTitle>
            <TitleYomi></TitleYomi>
            <TitleEN></TitleEN>
            <Comment></Comment>
            <Cat>1</Cat>
            <TitleFlag>0</TitleFlag>
            <FirstYear>2024</FirstYear>
            <FirstMonth>1</FirstMonth>
            <FirstEndYear></FirstEndYear>
            <FirstEndMonth></FirstEndMonth>
            <FirstCh></FirstCh>
            <Keywords></Keywords>
            <UserPoint></UserPoint>
            <UserPointRank></UserPointRank>
            <SubTitles></SubTitles>
        </TitleItem>
    </TitleItems>
</TitleLookupResponse>"#;

        // Act
        let response: TitleLookupResponse = quick_xml::de::from_str(xml).unwrap();

        // Assert
        assert!(response.result.is_none());
        let items = response.title_items.unwrap();
        assert_eq!(items.items.len(), 1);
        assert_eq!(items.items[0].tid, 100);
    }
}
