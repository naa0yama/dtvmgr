//! Chapter generation from structure analysis results and Trim information.
//!
//! Processes `obs_jlscp.txt` (structure analysis) and `obs_cut.avs` (Trim
//! commands) through a 3-stage pipeline:
//! 1. **`TrimReader`** — parse `Trim(start,end)` commands into frame positions
//! 2. **`CreateChapter`** — state-machine matching Trim positions against entries
//! 3. **`OutputData`** — write chapters in ORG, CUT, and `TVTPlay` formats

use std::fmt::Write as _;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use tracing::{debug, instrument};

/// Regex for `Trim(start,end)` AVS commands.
///
/// # Panics
///
/// Panics if the hard-coded regex pattern is invalid (impossible in practice).
pub(crate) static TRIM_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"Trim\((\d+),(\d+)\)").expect("valid trim regex")
});

/// Regex for structure analysis entries in `obs_jlscp.txt`.
///
/// # Panics
///
/// Panics if the hard-coded regex pattern is invalid (impossible in practice).
static JLSCP_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"^\s*(\d+)\s+(\d+)\s+(\d+)\s+[-\d]+\s+\d+.*:(\S+)").expect("valid jlscp regex")
});

// ── Types ────────────────────────────────────────────────────

/// A parsed entry from `obs_jlscp.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JlscpEntry {
    /// Start frame.
    pub frame_start: u32,
    /// End frame.
    pub frame_end: u32,
    /// Duration in seconds.
    pub duration_sec: u32,
    /// Structure comment (e.g. "CM", "Sponsor").
    pub comment: String,
}

/// Chapter type classification.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterType {
    /// Normal content (0).
    Normal,
    /// Explicit CM (1).
    Cm,
    /// Ambiguous — not clearly part or CM (2).
    Ambiguous,
    /// Standalone section (10).
    Standalone,
    /// Ambiguous standalone section (11).
    AmbiguousStandalone,
    /// Empty — zero duration (12).
    Empty,
}

impl ChapterType {
    /// Returns the numeric value matching the original JS implementation.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        match self {
            Self::Normal => 0,
            Self::Cm => 1,
            Self::Ambiguous => 2,
            Self::Standalone => 10,
            Self::AmbiguousStandalone => 11,
            Self::Empty => 12,
        }
    }

    /// Whether this type represents a standalone section (value >= 10).
    #[must_use]
    pub const fn is_standalone(self) -> bool {
        self.as_u32() >= 10
    }
}

/// A single chapter entry.
#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterEntry {
    /// Position in milliseconds.
    pub msec: u64,
    /// Whether this section is cut.
    pub cut: bool,
    /// Chapter display name.
    pub name: String,
}

// ── Helper functions ─────────────────────────────────────────

/// Convert a frame number to milliseconds at 29.97fps.
///
/// Formula: `floor((frame * 1001 + 15) / 30)`
#[allow(clippy::arithmetic_side_effects)]
#[must_use]
pub fn frame_to_msec(frame: u32) -> u64 {
    (u64::from(frame) * 1001 + 15) / 30
}

/// Convert a frame number to seconds at 29.97fps (high precision).
///
/// Formula: `frame * 1001.0 / 30000.0`
///
/// Used for VMAF sample positioning where sub-millisecond precision
/// is preferred over integer truncation.
#[must_use]
pub fn frame_to_secs(frame: u32) -> f64 {
    f64::from(frame) * 1001.0 / 30000.0
}

/// Convert a frame count to seconds at 29.97fps (for type classification).
///
/// Formula: `floor((frames * 1001 + 15000) / 30000)`
///
/// The result always fits in `u32` because the maximum input (`u32::MAX`)
/// produces a value well within `u32` range (about 143167 seconds).
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::as_conversions
)]
fn frame_to_sec(frames: u32) -> u32 {
    ((u64::from(frames) * 1001 + 15000) / 30000) as u32
}

/// Return the part letter for a given part number.
///
/// Cycles through `A`..`W` (23 letters) using `part % 23`.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::as_conversions
)]
#[must_use]
pub const fn part_letter(part: u32) -> char {
    // `part % 23` is always 0..=22, so adding to b'A' (65) stays within u8.
    (b'A' + (part % 23) as u8) as char
}

/// Classify a frame interval by its duration (`ProcChapterTypeTerm`).
#[must_use]
pub fn classify_by_duration(start: u32, end: u32) -> ChapterType {
    let sec = frame_to_sec(end.saturating_sub(start));
    match sec {
        0 => ChapterType::Empty,
        90 => ChapterType::AmbiguousStandalone,
        s if s < 15 => ChapterType::Ambiguous,
        _ => ChapterType::Normal,
    }
}

/// Classify by comment string (`ProcChapterTypeCmt`).
#[must_use]
pub fn classify_by_comment(comment: &str, duration_sec: u32) -> ChapterType {
    if comment.contains("Trailer") {
        if comment.contains("cut") {
            return ChapterType::Normal;
        }
        return ChapterType::Standalone;
    }
    if comment.contains("Sponsor")
        || comment.contains("Endcard")
        || comment.contains("Edge")
        || comment.contains("Border")
    {
        return ChapterType::AmbiguousStandalone;
    }
    if comment.contains("CM") {
        return ChapterType::Cm;
    }
    match duration_sec {
        90 => ChapterType::AmbiguousStandalone,
        60 => ChapterType::Standalone,
        s if s < 15 => ChapterType::Ambiguous,
        _ => ChapterType::Normal,
    }
}

/// Generate a chapter name (`ProcChapterName`).
#[allow(clippy::module_name_repetitions)]
#[must_use]
pub fn chapter_name(cut: bool, chapter_type: ChapterType, part: u32, duration_sec: u32) -> String {
    if cut {
        if chapter_type.is_standalone() {
            format!("X{duration_sec}Sec")
        } else if chapter_type == ChapterType::Cm {
            "XCM".to_owned()
        } else {
            "X".to_owned()
        }
    } else if chapter_type.is_standalone() {
        let letter = part_letter(part);
        format!("{letter}{duration_sec}Sec")
    } else {
        String::from(part_letter(part))
    }
}

// ── Stage 1: TrimReader ──────────────────────────────────────

/// Parse `Trim(start,end)` commands from AVS content.
///
/// Returns a flat vec: `[start0, end0+1, start1, end1+1, ...]`
///
/// # Panics
///
/// Panics if a captured digit group cannot be parsed as `u32` (should not
/// happen since the regex only captures `\d+`).
#[allow(clippy::arithmetic_side_effects, clippy::expect_used)]
#[must_use]
pub fn parse_trims(content: &str) -> Vec<u32> {
    let mut frames = Vec::new();

    for cap in TRIM_RE.captures_iter(content) {
        let start: u32 = cap[1].parse().expect("trim start is numeric");
        let end: u32 = cap[2].parse().expect("trim end is numeric");
        frames.push(start);
        frames.push(end + 1);
    }

    frames
}

/// Parse structure analysis entries from `obs_jlscp.txt` content.
///
/// # Panics
///
/// Panics if a captured digit group cannot be parsed as `u32` (should not
/// happen since the regex only captures `\d+`).
#[allow(clippy::expect_used)]
#[must_use]
pub fn parse_jlscp(content: &str) -> Vec<JlscpEntry> {
    let mut entries = Vec::new();

    for line in content.lines() {
        if let Some(cap) = JLSCP_RE.captures(line) {
            entries.push(JlscpEntry {
                frame_start: cap[1].parse().expect("jlscp frame_start is numeric"),
                frame_end: cap[2].parse().expect("jlscp frame_end is numeric"),
                duration_sec: cap[3].parse().expect("jlscp duration_sec is numeric"),
                comment: cap[4].to_owned(),
            });
        }
    }

    entries
}

// ── Stage 2: CreateChapter ───────────────────────────────────

/// Update `b_part_exist` for a non-cut chapter.
///
/// Ambiguous/`AmbiguousStandalone` types set to 1 (if unset); other non-empty
/// types set to 2.
fn update_part_non_cut(b_part_exist: &mut u32, chapter_type: ChapterType) {
    if chapter_type == ChapterType::AmbiguousStandalone || chapter_type == ChapterType::Ambiguous {
        if *b_part_exist == 0 {
            *b_part_exist = 1;
        }
    } else if chapter_type != ChapterType::Empty {
        *b_part_exist = 2;
    }
}

/// Update `b_part_exist` / `n_part` for a cut chapter.
///
/// If content existed before this cut and the type is non-empty, increment part
/// and reset.
#[allow(clippy::arithmetic_side_effects)]
fn update_part_cut(b_part_exist: &mut u32, n_part: &mut u32, chapter_type: ChapterType) {
    if *b_part_exist > 0 && chapter_type != ChapterType::Empty {
        *n_part += 1;
        *b_part_exist = 0;
    }
}

/// Create chapter entries from Trim frame positions and structure analysis.
///
/// Implements the state-machine algorithm from the original `chapter_jls.js`.
#[instrument(skip_all)]
#[allow(
    clippy::too_many_lines,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    unused_assignments
)]
#[must_use]
pub fn create_chapters(trims: &[u32], entries: &[JlscpEntry]) -> Vec<ChapterEntry> {
    const FRAME_MARGIN: u32 = 30;

    let n_trim_total = trims.len();
    let mut chapters: Vec<ChapterEntry> = Vec::new();

    // Initial state: skip first Trim if it starts within margin of frame 0
    let mut n_trim_num: usize =
        usize::from(!trims.is_empty() && trims.first().is_some_and(|&t| t <= FRAME_MARGIN));
    let mut n_frm_begin: u32 = 0;
    let mut n_part: u32 = 0;
    let mut b_part_exist: u32 = 0;
    let mut n_last_type: ChapterType = ChapterType::Normal;

    for entry in entries {
        let n_frm_st = entry.frame_start;
        let n_frm_ed = entry.frame_end;
        let n_sec_rd = entry.duration_sec;
        let str_cmt = &entry.comment;

        // Inner loop: process Trim boundaries before entry end
        while n_trim_num < n_trim_total && trims[n_trim_num] < n_frm_ed.saturating_sub(FRAME_MARGIN)
        {
            let n_frm_trim = trims[n_trim_num];
            let b_cut_on = (n_trim_num + 1) % 2 == 1;
            let term_type = classify_by_duration(n_frm_begin, n_frm_trim);
            let name = chapter_name(
                b_cut_on,
                term_type,
                n_part,
                frame_to_sec(n_frm_trim.saturating_sub(n_frm_begin)),
            );

            // Part update for Trim boundary
            if b_cut_on {
                update_part_cut(&mut b_part_exist, &mut n_part, term_type);
            } else {
                update_part_non_cut(&mut b_part_exist, term_type);
            }

            chapters.push(ChapterEntry {
                msec: frame_to_msec(n_frm_trim),
                cut: b_cut_on,
                name,
            });

            n_frm_begin = n_frm_trim;
            n_trim_num += 1;
        }

        // Check if Trim boundary aligns with entry end
        let b_show_on = if n_trim_num < n_trim_total && trims[n_trim_num] <= n_frm_ed + FRAME_MARGIN
        {
            // Advance past the aligned Trim boundary
            n_trim_num += 1;
            true
        } else {
            false
        };

        // ProcChapterTypeCmt
        let n_type = classify_by_comment(str_cmt, n_sec_rd);
        let b_cut_on = (n_trim_num + 1) % 2 == 1;

        // Cut boundary logic
        if b_show_on {
            if b_cut_on {
                // Entering cut: insert chapter at entry start
                let name = chapter_name(
                    false,
                    n_last_type,
                    n_part,
                    frame_to_sec(n_frm_st.saturating_sub(n_frm_begin)),
                );
                chapters.push(ChapterEntry {
                    msec: frame_to_msec(n_frm_st),
                    cut: false,
                    name,
                });

                // Part update for non-cut
                update_part_non_cut(&mut b_part_exist, n_last_type);

                n_frm_begin = n_frm_st;

                // Insert at entry end
                let name = chapter_name(true, n_type, n_part, n_sec_rd);
                chapters.push(ChapterEntry {
                    msec: frame_to_msec(n_frm_ed),
                    cut: true,
                    name,
                });

                // Part update for cut
                update_part_cut(&mut b_part_exist, &mut n_part, n_type);
            } else {
                // Exiting cut: insert chapter at entry start
                let name = chapter_name(
                    true,
                    n_last_type,
                    n_part,
                    frame_to_sec(n_frm_st.saturating_sub(n_frm_begin)),
                );
                chapters.push(ChapterEntry {
                    msec: frame_to_msec(n_frm_st),
                    cut: true,
                    name,
                });

                // Part update for cut
                update_part_cut(&mut b_part_exist, &mut n_part, n_last_type);

                n_frm_begin = n_frm_st;

                // Insert at entry end
                let name = chapter_name(false, n_type, n_part, n_sec_rd);
                chapters.push(ChapterEntry {
                    msec: frame_to_msec(n_frm_ed),
                    cut: false,
                    name,
                });

                // Part update for non-cut
                update_part_non_cut(&mut b_part_exist, n_type);
            }
            n_frm_begin = n_frm_ed;
        } else {
            // No Trim boundary alignment — just track type
            if !b_cut_on {
                update_part_non_cut(&mut b_part_exist, n_type);
            }
        }

        n_last_type = n_type;
    }

    chapters
}

// ── Stage 3: OutputData ──────────────────────────────────────

/// Minimum millisecond gap between chapters for dedup.
const MSEC_DIVMIN: u64 = 100;

/// Format milliseconds as `HH:MM:SS.mmm`.
#[allow(clippy::arithmetic_side_effects)]
fn format_time(msec: u64) -> String {
    let total_sec = msec / 1000;
    let ms = msec % 1000;
    let hours = total_sec / 3600;
    let minutes = (total_sec % 3600) / 60;
    let seconds = total_sec % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{ms:03}")
}

/// Write FFMETADATA1 chapter file with all sections.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
#[instrument(skip_all, err(level = "error"))]
pub fn write_org(output_path: &Path, chapters: &[ChapterEntry]) -> Result<()> {
    write_ffmetadata(output_path, chapters, false)
}

/// Write FFMETADATA1 chapter file with only non-cut sections.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
#[instrument(skip_all, err(level = "error"))]
pub fn write_cut(output_path: &Path, chapters: &[ChapterEntry]) -> Result<()> {
    write_ffmetadata(output_path, chapters, true)
}

/// Write FFMETADATA1 format chapters.
#[allow(clippy::arithmetic_side_effects)]
fn write_ffmetadata(output_path: &Path, chapters: &[ChapterEntry], skip_cut: bool) -> Result<()> {
    let mut content = String::from(";FFMETADATA1\n");
    let mut last_msec: Option<u64> = None;

    for ch in chapters {
        if skip_cut && ch.cut {
            continue;
        }
        // Dedup: skip if too close to previous
        if let Some(prev) = last_msec
            && ch.msec.saturating_sub(prev) < MSEC_DIVMIN
        {
            continue;
        }
        last_msec = Some(ch.msec);

        write!(
            content,
            "\n[CHAPTER]\nTIMEBASE=1/1000\n# {}\nSTART={}\nEND={}\ntitle={}\n",
            format_time(ch.msec),
            ch.msec,
            ch.msec + 1,
            ch.name,
        )
        .with_context(|| "failed to format chapter entry")?;
    }

    std::fs::write(output_path, &content)
        .with_context(|| format!("failed to write chapter file: {}", output_path.display()))?;

    debug!(path = %output_path.display(), "wrote chapter file");
    Ok(())
}

/// Write `TVTPlay` format chapter file.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
#[instrument(skip_all, err(level = "error"))]
pub fn write_tvt(output_path: &Path, chapters: &[ChapterEntry]) -> Result<()> {
    let mut content = String::from("c-");
    let mut last_msec: u64 = 0;
    let mut prev_cut = false;
    let mut last_written_msec: Option<u64> = None;

    for ch in chapters {
        // Dedup
        if let Some(prev) = last_written_msec
            && ch.msec.saturating_sub(prev) < MSEC_DIVMIN
        {
            prev_cut = ch.cut;
            continue;
        }
        last_written_msec = Some(ch.msec);

        let delta = ch.msec.saturating_sub(last_msec);
        last_msec = ch.msec;

        // Replace ASCII hyphen with fullwidth in name
        let safe_name = ch.name.replace('-', "\u{FF0D}");

        // Prefix for cut transitions
        let prefix = if ch.cut && !prev_cut {
            "ix"
        } else if !ch.cut && prev_cut {
            "ox"
        } else {
            ""
        };

        write!(content, "{delta}c{prefix}{safe_name}-")
            .with_context(|| "failed to format TVTPlay entry")?;

        prev_cut = ch.cut;
    }

    // Trailer: end marker
    let tvto = if prev_cut { "ix" } else { "" };
    write!(content, "0e{tvto}-c").with_context(|| "failed to format TVTPlay trailer")?;

    std::fs::write(output_path, &content)
        .with_context(|| format!("failed to write TVTPlay file: {}", output_path.display()))?;

    debug!(path = %output_path.display(), "wrote TVTPlay chapter file");
    Ok(())
}

/// Write all three chapter formats at once.
///
/// # Errors
///
/// Returns an error if any file cannot be written.
#[instrument(skip_all, err(level = "error"))]
pub fn write_all(chapters: &[ChapterEntry], org: &Path, cut: &Path, tvt: &Path) -> Result<()> {
    write_org(org, chapters).with_context(|| "failed to write ORG chapters")?;
    write_cut(cut, chapters).with_context(|| "failed to write CUT chapters")?;
    write_tvt(tvt, chapters).with_context(|| "failed to write TVT chapters")?;
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    // ── frame_to_msec ────────────────────────────────────────

    #[test]
    fn test_frame_to_msec_zero() {
        assert_eq!(frame_to_msec(0), 0);
    }

    #[test]
    fn test_frame_to_msec_one() {
        // (1 * 1001 + 15) / 30 = 1016 / 30 = 33
        assert_eq!(frame_to_msec(1), 33);
    }

    #[test]
    fn test_frame_to_msec_30() {
        // (30 * 1001 + 15) / 30 = 30045 / 30 = 1001
        assert_eq!(frame_to_msec(30), 1001);
    }

    #[test]
    fn test_frame_to_msec_900() {
        // (900 * 1001 + 15) / 30 = 900915 / 30 = 30030
        assert_eq!(frame_to_msec(900), 30030);
    }

    // ── frame_to_sec ─────────────────────────────────────────

    #[test]
    fn test_frame_to_sec_zero() {
        assert_eq!(frame_to_sec(0), 0);
    }

    #[test]
    fn test_frame_to_sec_30_frames_is_1_sec() {
        // (30 * 1001 + 15000) / 30000 = 45030 / 30000 = 1
        assert_eq!(frame_to_sec(30), 1);
    }

    #[test]
    fn test_frame_to_sec_2700_frames_is_90_sec() {
        // 2700 * 1001 + 15000 = 2717700 / 30000 = 90
        assert_eq!(frame_to_sec(2700), 90);
    }

    // ── part_letter ──────────────────────────────────────────

    #[test]
    fn test_part_letter_zero_is_a() {
        assert_eq!(part_letter(0), 'A');
    }

    #[test]
    fn test_part_letter_one_is_b() {
        assert_eq!(part_letter(1), 'B');
    }

    #[test]
    fn test_part_letter_22_is_w() {
        assert_eq!(part_letter(22), 'W');
    }

    #[test]
    fn test_part_letter_wraps_at_23() {
        assert_eq!(part_letter(23), 'A');
        assert_eq!(part_letter(24), 'B');
    }

    // ── classify_by_duration ─────────────────────────────────

    #[test]
    fn test_classify_by_duration_zero_sec_is_empty() {
        assert_eq!(classify_by_duration(0, 0), ChapterType::Empty);
    }

    #[test]
    fn test_classify_by_duration_short_is_ambiguous() {
        // 10 sec = ~300 frames
        assert_eq!(classify_by_duration(0, 300), ChapterType::Ambiguous);
    }

    #[test]
    fn test_classify_by_duration_90_sec_is_ambiguous_standalone() {
        assert_eq!(
            classify_by_duration(0, 2700),
            ChapterType::AmbiguousStandalone
        );
    }

    #[test]
    fn test_classify_by_duration_normal() {
        // 30 sec = ~900 frames
        assert_eq!(classify_by_duration(0, 900), ChapterType::Normal);
    }

    // ── classify_by_comment ──────────────────────────────────

    #[test]
    fn test_classify_trailer_cut_is_normal() {
        assert_eq!(classify_by_comment("Trailer(cut)", 30), ChapterType::Normal);
    }

    #[test]
    fn test_classify_trailer_is_standalone() {
        assert_eq!(classify_by_comment("Trailer", 30), ChapterType::Standalone);
    }

    #[test]
    fn test_classify_sponsor_is_ambiguous_standalone() {
        assert_eq!(
            classify_by_comment("Sponsor", 30),
            ChapterType::AmbiguousStandalone
        );
    }

    #[test]
    fn test_classify_endcard_is_ambiguous_standalone() {
        assert_eq!(
            classify_by_comment("Endcard", 30),
            ChapterType::AmbiguousStandalone
        );
    }

    #[test]
    fn test_classify_edge_is_ambiguous_standalone() {
        assert_eq!(
            classify_by_comment("Edge", 10),
            ChapterType::AmbiguousStandalone
        );
    }

    #[test]
    fn test_classify_border_is_ambiguous_standalone() {
        assert_eq!(
            classify_by_comment("Border", 10),
            ChapterType::AmbiguousStandalone
        );
    }

    #[test]
    fn test_classify_cm_is_cm() {
        assert_eq!(classify_by_comment("CM", 30), ChapterType::Cm);
    }

    #[test]
    fn test_classify_90sec_is_ambiguous_standalone() {
        assert_eq!(
            classify_by_comment("other", 90),
            ChapterType::AmbiguousStandalone
        );
    }

    #[test]
    fn test_classify_60sec_is_standalone() {
        assert_eq!(classify_by_comment("other", 60), ChapterType::Standalone);
    }

    #[test]
    fn test_classify_short_is_ambiguous() {
        assert_eq!(classify_by_comment("other", 10), ChapterType::Ambiguous);
    }

    #[test]
    fn test_classify_other_is_normal() {
        assert_eq!(classify_by_comment("other", 30), ChapterType::Normal);
    }

    // ── chapter_name ─────────────────────────────────────────

    #[test]
    fn test_chapter_name_non_cut_normal() {
        assert_eq!(chapter_name(false, ChapterType::Normal, 0, 30), "A");
    }

    #[test]
    fn test_chapter_name_non_cut_standalone() {
        assert_eq!(
            chapter_name(false, ChapterType::Standalone, 1, 60),
            "B60Sec"
        );
    }

    #[test]
    fn test_chapter_name_non_cut_ambiguous_standalone() {
        assert_eq!(
            chapter_name(false, ChapterType::AmbiguousStandalone, 0, 90),
            "A90Sec"
        );
    }

    #[test]
    fn test_chapter_name_cut_cm() {
        assert_eq!(chapter_name(true, ChapterType::Cm, 0, 30), "XCM");
    }

    #[test]
    fn test_chapter_name_cut_standalone() {
        assert_eq!(chapter_name(true, ChapterType::Standalone, 0, 60), "X60Sec");
    }

    #[test]
    fn test_chapter_name_cut_normal() {
        assert_eq!(chapter_name(true, ChapterType::Normal, 0, 30), "X");
    }

    // ── ChapterType ──────────────────────────────────────────

    #[test]
    fn test_chapter_type_is_standalone() {
        assert!(!ChapterType::Normal.is_standalone());
        assert!(!ChapterType::Cm.is_standalone());
        assert!(!ChapterType::Ambiguous.is_standalone());
        assert!(ChapterType::Standalone.is_standalone());
        assert!(ChapterType::AmbiguousStandalone.is_standalone());
        assert!(ChapterType::Empty.is_standalone());
    }

    // ── parse_trims ──────────────────────────────────────────

    #[test]
    fn test_parse_trims_single() {
        let result = parse_trims("Trim(100,500)");
        assert_eq!(result, vec![100, 501]);
    }

    #[test]
    fn test_parse_trims_multiple() {
        let result = parse_trims("Trim(100,500)Trim(800,1200)");
        assert_eq!(result, vec![100, 501, 800, 1201]);
    }

    #[test]
    fn test_parse_trims_empty() {
        let result = parse_trims("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_trims_no_match() {
        let result = parse_trims("LWLibavVideoSource(TSFilePath)");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_trims_multiline() {
        let content = "Import(\"in_org.avs\")\nTrim(0,1000)\nTrim(2000,3000)";
        let result = parse_trims(content);
        assert_eq!(result, vec![0, 1001, 2000, 3001]);
    }

    // ── parse_jlscp ─────────────────────────────────────────

    #[test]
    fn test_parse_jlscp_basic() {
        let content = "    100  500  13  0  0  nLogos:CM\n";
        let result = parse_jlscp(content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].frame_start, 100);
        assert_eq!(result[0].frame_end, 500);
        assert_eq!(result[0].duration_sec, 13);
        assert_eq!(result[0].comment, "CM");
    }

    #[test]
    fn test_parse_jlscp_multiple() {
        let content = "\
    100  500  13  0  0  nLogos:CM
   501  2000  50  -1  1  nLogos:Nope";
        let result = parse_jlscp(content);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].comment, "CM");
        assert_eq!(result[1].comment, "Nope");
    }

    #[test]
    fn test_parse_jlscp_invalid_line_skipped() {
        let content = "This is not a valid line\n    100  500  13  0  0  nLogos:CM";
        let result = parse_jlscp(content);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_parse_jlscp_empty() {
        assert!(parse_jlscp("").is_empty());
    }

    #[test]
    fn test_parse_jlscp_negative_field() {
        let content = "    100  500  13  -2  0  nLogos:Sponsor";
        let result = parse_jlscp(content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].comment, "Sponsor");
    }

    // ── create_chapters ──────────────────────────────────────

    #[test]
    fn test_create_chapters_empty_input() {
        let result = create_chapters(&[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_create_chapters_empty_trims() {
        let entries = vec![JlscpEntry {
            frame_start: 0,
            frame_end: 900,
            duration_sec: 30,
            comment: "nLogos".to_owned(),
        }];
        let result = create_chapters(&[], &entries);
        assert!(result.is_empty());
    }

    #[test]
    fn test_create_chapters_basic_cm_then_main() {
        // Scenario: CM segment followed by main content
        let trims = vec![900, 2700]; // Start at 900, end+1 at 2700
        let entries = vec![
            JlscpEntry {
                frame_start: 0,
                frame_end: 900,
                duration_sec: 30,
                comment: "CM".to_owned(),
            },
            JlscpEntry {
                frame_start: 900,
                frame_end: 2700,
                duration_sec: 60,
                comment: "nLogos".to_owned(),
            },
        ];
        let result = create_chapters(&trims, &entries);
        // Should produce chapters at the CM/main boundary
        assert!(!result.is_empty());
    }

    #[test]
    fn test_create_chapters_all_non_cut() {
        // Single Trim spanning the entire content
        let trims = vec![0, 2700];
        let entries = vec![JlscpEntry {
            frame_start: 0,
            frame_end: 2700,
            duration_sec: 90,
            comment: "nLogos".to_owned(),
        }];
        let result = create_chapters(&trims, &entries);
        // With the Trim starting at 0 (<=30), nTrimNum starts at 1
        // Trim boundary at 2700 aligns with entry end
        assert!(!result.is_empty());
    }

    // ── format_time ──────────────────────────────────────────

    #[test]
    fn test_format_time_zero() {
        assert_eq!(format_time(0), "00:00:00.000");
    }

    #[test]
    fn test_format_time_example() {
        assert_eq!(format_time(83456), "00:01:23.456");
    }

    #[test]
    fn test_format_time_large() {
        // 1h 30m 45s 678ms = 5445678ms
        assert_eq!(format_time(5_445_678), "01:30:45.678");
    }

    // ── write_org ────────────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_write_org_header() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("org.txt");
        let chapters = vec![ChapterEntry {
            msec: 0,
            cut: false,
            name: "A".to_owned(),
        }];

        // Act
        write_org(&path, &chapters).unwrap();

        // Assert
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with(";FFMETADATA1\n"));
        assert!(content.contains("[CHAPTER]"));
        assert!(content.contains("TIMEBASE=1/1000"));
        assert!(content.contains("START=0"));
        assert!(content.contains("END=1"));
        assert!(content.contains("title=A"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_write_org_includes_cut_sections() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("org.txt");
        let chapters = vec![
            ChapterEntry {
                msec: 0,
                cut: false,
                name: "A".to_owned(),
            },
            ChapterEntry {
                msec: 30000,
                cut: true,
                name: "XCM".to_owned(),
            },
        ];

        // Act
        write_org(&path, &chapters).unwrap();

        // Assert
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("title=A"));
        assert!(content.contains("title=XCM"));
    }

    // ── write_cut ────────────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_write_cut_excludes_cut_sections() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("cut.txt");
        let chapters = vec![
            ChapterEntry {
                msec: 0,
                cut: false,
                name: "A".to_owned(),
            },
            ChapterEntry {
                msec: 30000,
                cut: true,
                name: "XCM".to_owned(),
            },
        ];

        // Act
        write_cut(&path, &chapters).unwrap();

        // Assert
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("title=A"));
        assert!(!content.contains("title=XCM"));
    }

    // ── write_tvt ────────────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_write_tvt_basic() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tvt.chapter");
        let chapters = vec![
            ChapterEntry {
                msec: 0,
                cut: false,
                name: "A".to_owned(),
            },
            ChapterEntry {
                msec: 30000,
                cut: false,
                name: "B".to_owned(),
            },
        ];

        // Act
        write_tvt(&path, &chapters).unwrap();

        // Assert
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("c-"));
        assert!(content.ends_with("0e-c"));
        assert!(content.contains("0cA-"));
        assert!(content.contains("30000cB-"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_write_tvt_cut_transitions() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tvt.chapter");
        let chapters = vec![
            ChapterEntry {
                msec: 0,
                cut: false,
                name: "A".to_owned(),
            },
            ChapterEntry {
                msec: 30000,
                cut: true,
                name: "XCM".to_owned(),
            },
            ChapterEntry {
                msec: 60000,
                cut: false,
                name: "B".to_owned(),
            },
        ];

        // Act
        write_tvt(&path, &chapters).unwrap();

        // Assert
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("ixXCM"));
        assert!(content.contains("oxB"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_write_tvt_hyphen_replaced() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tvt.chapter");
        let chapters = vec![ChapterEntry {
            msec: 0,
            cut: false,
            name: "A-B".to_owned(),
        }];

        // Act
        write_tvt(&path, &chapters).unwrap();

        // Assert
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("A\u{FF0D}B"));
        assert!(!content.contains("A-B"));
    }

    // ── write_all ────────────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_write_all_creates_three_files() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let org = tmp.path().join("org.txt");
        let cut = tmp.path().join("cut.txt");
        let tvt = tmp.path().join("tvt.chapter");
        let chapters = vec![ChapterEntry {
            msec: 0,
            cut: false,
            name: "A".to_owned(),
        }];

        // Act
        write_all(&chapters, &org, &cut, &tvt).unwrap();

        // Assert
        assert!(org.exists());
        assert!(cut.exists());
        assert!(tvt.exists());
    }

    // ── dedup ────────────────────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_write_org_dedup_close_chapters() {
        // Arrange
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("org.txt");
        let chapters = vec![
            ChapterEntry {
                msec: 1000,
                cut: false,
                name: "A".to_owned(),
            },
            ChapterEntry {
                msec: 1050,
                cut: false,
                name: "B".to_owned(),
            },
            ChapterEntry {
                msec: 2000,
                cut: false,
                name: "C".to_owned(),
            },
        ];

        // Act
        write_org(&path, &chapters).unwrap();

        // Assert
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("title=A"));
        assert!(!content.contains("title=B")); // deduped
        assert!(content.contains("title=C"));
    }

    // ── classify_by_duration (additional) ────────────────────────

    #[test]
    fn test_classify_by_duration_boundary_15_sec() {
        // 15 sec → Normal (boundary: >= 15 is Normal)
        // 15 sec = 450 frames at 30fps
        assert_eq!(classify_by_duration(0, 450), ChapterType::Normal);
    }

    #[test]
    fn test_classify_by_duration_boundary_14_sec() {
        // 14 sec → Ambiguous (< 15)
        assert_eq!(classify_by_duration(0, 420), ChapterType::Ambiguous);
    }

    // ── classify_by_comment (additional) ─────────────────────────

    #[test]
    fn test_classify_by_comment_trailer_with_cut() {
        assert_eq!(classify_by_comment("Trailer cut", 30), ChapterType::Normal);
    }

    #[test]
    fn test_classify_by_comment_trailer_without_cut() {
        assert_eq!(classify_by_comment("Trailer", 30), ChapterType::Standalone);
    }

    #[test]
    fn test_classify_by_comment_sponsor() {
        assert_eq!(
            classify_by_comment("Sponsor", 30),
            ChapterType::AmbiguousStandalone
        );
    }

    #[test]
    fn test_classify_by_comment_endcard() {
        assert_eq!(
            classify_by_comment("Endcard", 30),
            ChapterType::AmbiguousStandalone
        );
    }

    #[test]
    fn test_classify_by_comment_edge() {
        assert_eq!(
            classify_by_comment("Edge", 30),
            ChapterType::AmbiguousStandalone
        );
    }

    #[test]
    fn test_classify_by_comment_border() {
        assert_eq!(
            classify_by_comment("Border", 30),
            ChapterType::AmbiguousStandalone
        );
    }

    #[test]
    fn test_classify_by_comment_cm() {
        assert_eq!(classify_by_comment("CM", 30), ChapterType::Cm);
    }

    #[test]
    fn test_classify_by_comment_duration_60() {
        assert_eq!(classify_by_comment("unknown", 60), ChapterType::Standalone);
    }

    #[test]
    fn test_classify_by_comment_duration_short() {
        assert_eq!(classify_by_comment("unknown", 5), ChapterType::Ambiguous);
    }

    #[test]
    fn test_classify_by_comment_duration_normal() {
        assert_eq!(classify_by_comment("unknown", 30), ChapterType::Normal);
    }

    // ── chapter_name (additional) ────────────────────────────────

    #[test]
    fn test_chapter_name_cut_ambiguous() {
        assert_eq!(chapter_name(true, ChapterType::Ambiguous, 0, 10), "X");
    }

    #[test]
    fn test_chapter_name_cut_ambiguous_standalone() {
        assert_eq!(
            chapter_name(true, ChapterType::AmbiguousStandalone, 0, 90),
            "X90Sec"
        );
    }

    #[test]
    fn test_chapter_name_noncut_standalone() {
        assert_eq!(
            chapter_name(false, ChapterType::Standalone, 0, 60),
            "A60Sec"
        );
    }

    #[test]
    fn test_chapter_name_noncut_normal() {
        assert_eq!(chapter_name(false, ChapterType::Normal, 0, 30), "A");
    }

    #[test]
    fn test_chapter_name_noncut_normal_part1() {
        assert_eq!(chapter_name(false, ChapterType::Normal, 1, 30), "B");
    }

    // ── update_part_non_cut / update_part_cut ────────────────────

    #[test]
    fn test_update_part_non_cut_ambiguous_sets_one() {
        let mut b = 0;
        update_part_non_cut(&mut b, ChapterType::Ambiguous);
        assert_eq!(b, 1);
    }

    #[test]
    fn test_update_part_non_cut_ambiguous_no_overwrite() {
        let mut b = 2;
        update_part_non_cut(&mut b, ChapterType::Ambiguous);
        assert_eq!(b, 2); // does not overwrite when already > 0
    }

    #[test]
    fn test_update_part_non_cut_ambiguous_standalone_sets_one() {
        let mut b = 0;
        update_part_non_cut(&mut b, ChapterType::AmbiguousStandalone);
        assert_eq!(b, 1);
    }

    #[test]
    fn test_update_part_non_cut_normal_sets_two() {
        let mut b = 0;
        update_part_non_cut(&mut b, ChapterType::Normal);
        assert_eq!(b, 2);
    }

    #[test]
    fn test_update_part_non_cut_empty_noop() {
        let mut b = 0;
        update_part_non_cut(&mut b, ChapterType::Empty);
        assert_eq!(b, 0);
    }

    #[test]
    fn test_update_part_cut_increments_when_active() {
        let mut b = 1;
        let mut n = 0;
        update_part_cut(&mut b, &mut n, ChapterType::Normal);
        assert_eq!(n, 1);
        assert_eq!(b, 0);
    }

    #[test]
    fn test_update_part_cut_noop_when_zero() {
        let mut b = 0;
        let mut n = 0;
        update_part_cut(&mut b, &mut n, ChapterType::Normal);
        assert_eq!(n, 0);
        assert_eq!(b, 0);
    }

    #[test]
    fn test_update_part_cut_noop_when_empty_type() {
        let mut b = 1;
        let mut n = 0;
        update_part_cut(&mut b, &mut n, ChapterType::Empty);
        assert_eq!(n, 0);
        assert_eq!(b, 1);
    }

    // ── frame_to_secs ────────────────────────────────────────

    #[test]
    fn test_frame_to_secs_zero() {
        // Arrange / Act
        let secs = frame_to_secs(0);

        // Assert
        assert!((secs - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_frame_to_secs_one() {
        // Arrange / Act — 1 * 1001 / 30000 ≈ 0.03337
        let secs = frame_to_secs(1);

        // Assert
        let expected = 1001.0 / 30000.0;
        assert!(
            (secs - expected).abs() < 1e-9,
            "frame 1: got {secs}, expected {expected}"
        );
    }

    #[test]
    fn test_frame_to_secs_hundred() {
        // Arrange / Act — 100 * 1001 / 30000 ≈ 3.3367
        let secs = frame_to_secs(100);

        // Assert
        let expected = 100.0 * 1001.0 / 30000.0;
        assert!(
            (secs - expected).abs() < 1e-9,
            "frame 100: got {secs}, expected {expected}"
        );
    }

    #[test]
    fn test_frame_to_secs_30000() {
        // Arrange / Act — 30000 * 1001 / 30000 = 1001.0
        let secs = frame_to_secs(30000);

        // Assert
        assert!(
            (secs - 1001.0).abs() < 1e-9,
            "frame 30000: got {secs}, expected 1001.0"
        );
    }
}
