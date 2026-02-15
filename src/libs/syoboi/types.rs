//! Syoboi Calendar API response types.

use serde::Deserialize;

use super::xml::{
    deserialize_empty_string_as_none, deserialize_empty_string_as_none_i32,
    deserialize_empty_string_as_none_u32,
};

/// A single title from `TitleLookup` response.
#[derive(Debug, Clone, Deserialize)]
#[allow(clippy::module_name_repetitions)]
pub struct SyoboiTitle {
    /// Title ID.
    #[serde(rename = "TID")]
    pub tid: u32,
    /// Last update timestamp (e.g. "2022-06-30 01:56:20").
    #[serde(rename = "LastUpdate")]
    pub last_update: String,
    /// Title name.
    #[serde(rename = "Title")]
    pub title: String,
    /// Short title (may be empty).
    #[serde(
        rename = "ShortTitle",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub short_title: Option<String>,
    /// Title reading (hiragana).
    #[serde(
        rename = "TitleYomi",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub title_yomi: Option<String>,
    /// English title (may be empty).
    #[serde(
        rename = "TitleEN",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub title_en: Option<String>,
    /// Free-form comment (staff, cast, etc.).
    #[serde(
        rename = "Comment",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub comment: Option<String>,
    /// Category (10=anime, etc.).
    #[serde(
        rename = "Cat",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub cat: Option<u32>,
    /// Title flag.
    #[serde(
        rename = "TitleFlag",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub title_flag: Option<u32>,
    /// First broadcast year.
    #[serde(
        rename = "FirstYear",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub first_year: Option<u32>,
    /// First broadcast month.
    #[serde(
        rename = "FirstMonth",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub first_month: Option<u32>,
    /// Last broadcast year.
    #[serde(
        rename = "FirstEndYear",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub first_end_year: Option<u32>,
    /// Last broadcast month.
    #[serde(
        rename = "FirstEndMonth",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub first_end_month: Option<u32>,
    /// Earliest broadcast channel.
    #[serde(
        rename = "FirstCh",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub first_ch: Option<String>,
    /// Keywords.
    #[serde(
        rename = "Keywords",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub keywords: Option<String>,
    /// User point score.
    #[serde(
        rename = "UserPoint",
        deserialize_with = "deserialize_empty_string_as_none_i32",
        default
    )]
    pub user_point: Option<i32>,
    /// User point rank.
    #[serde(
        rename = "UserPointRank",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub user_point_rank: Option<u32>,
    /// Raw subtitle text ("*01*Subtitle\n*02*Subtitle" format).
    #[serde(
        rename = "SubTitles",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub sub_titles: Option<String>,
}

/// A single program from `ProgLookup` response.
#[derive(Debug, Clone, Deserialize)]
#[allow(clippy::module_name_repetitions)]
pub struct SyoboiProgram {
    /// Program ID.
    #[serde(rename = "PID")]
    pub pid: u32,
    /// Title ID.
    #[serde(rename = "TID")]
    pub tid: u32,
    /// Broadcast start time (e.g. "2022-04-09 23:00:00").
    #[serde(rename = "StTime")]
    pub st_time: String,
    /// Start offset in seconds.
    #[serde(
        rename = "StOffset",
        deserialize_with = "deserialize_empty_string_as_none_i32",
        default
    )]
    pub st_offset: Option<i32>,
    /// Broadcast end time.
    #[serde(rename = "EdTime")]
    pub ed_time: String,
    /// Episode number (0 = special/unset).
    #[serde(
        rename = "Count",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub count: Option<u32>,
    /// Subtitle (may be empty; prefer `st_sub_title`).
    #[serde(
        rename = "SubTitle",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub sub_title: Option<String>,
    /// Program comment.
    #[serde(
        rename = "ProgComment",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub prog_comment: Option<String>,
    /// Flag bitmask (2=first episode, etc.).
    #[serde(
        rename = "Flag",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub flag: Option<u32>,
    /// Deleted flag.
    #[serde(
        rename = "Deleted",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub deleted: Option<u32>,
    /// Warning flag.
    #[serde(
        rename = "Warn",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub warn: Option<u32>,
    /// Channel ID.
    #[serde(rename = "ChID")]
    pub ch_id: u32,
    /// Revision number.
    #[serde(
        rename = "Revision",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub revision: Option<u32>,
    /// Last update timestamp.
    #[serde(
        rename = "LastUpdate",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub last_update: Option<String>,
    /// Subtitle from `SubTitles` table join (only with `JOIN=SubTitles`).
    #[serde(
        rename = "STSubTitle",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub st_sub_title: Option<String>,
}

/// A single channel from `ChLookup` response.
#[derive(Debug, Clone, Deserialize)]
#[allow(clippy::module_name_repetitions)]
pub struct SyoboiChannel {
    /// Channel ID.
    #[serde(rename = "ChID")]
    pub ch_id: u32,
    /// Channel group ID.
    #[serde(
        rename = "ChGID",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub ch_gid: Option<u32>,
    /// Channel name.
    #[serde(rename = "ChName")]
    pub ch_name: String,
    /// Channel comment.
    #[serde(
        rename = "ChComment",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub ch_comment: Option<String>,
    /// Channel URL.
    #[serde(
        rename = "ChURL",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub ch_url: Option<String>,
    /// Last update timestamp.
    #[serde(
        rename = "LastUpdate",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub last_update: Option<String>,
    /// EPG channel name.
    #[serde(
        rename = "ChiEPGName",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub ch_iepg_name: Option<String>,
    /// EPG URL.
    #[serde(
        rename = "ChEPGURL",
        deserialize_with = "deserialize_empty_string_as_none",
        default
    )]
    pub ch_epg_url: Option<String>,
    /// Channel number.
    #[serde(
        rename = "ChNumber",
        deserialize_with = "deserialize_empty_string_as_none_u32",
        default
    )]
    pub ch_number: Option<u32>,
}
