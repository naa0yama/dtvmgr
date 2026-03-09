//! Normalize viewer TUI state management.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::LazyLock;

use ratatui::widgets::TableState;
use regex::Regex;
use unicode_normalization::UnicodeNormalization;

/// Fallback regex to extract season number from trimmed text.
#[allow(clippy::expect_used)]
static SEASON_NUM_FALLBACK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d+").expect("fallback regex must compile"));

// ---------------------------------------------------------------------------
// MediaType
// ---------------------------------------------------------------------------

/// Media type inferred from the Syoboi category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    /// TV series (anime, tokusatsu, etc.).
    Tv,
    /// Theatrical movie.
    Movie,
    /// OVA / ONA.
    Ova,
}

impl fmt::Display for MediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tv => write!(f, "Tv"),
            Self::Movie => write!(f, "Movie"),
            Self::Ova => write!(f, "Ova"),
        }
    }
}

/// Maps Syoboi `Cat` value to `MediaType`.
#[must_use]
pub const fn categorize(cat: Option<u32>) -> MediaType {
    match cat {
        Some(7) => MediaType::Ova,
        Some(8) => MediaType::Movie,
        _ => MediaType::Tv,
    }
}

// ---------------------------------------------------------------------------
// Character normalization
// ---------------------------------------------------------------------------

/// Maps characters that NFKC does not normalize to the desired form.
const fn pre_nfkc_normalize(ch: char) -> char {
    match ch {
        // Wave dash -> tilde
        '\u{301C}' => '~',
        // Various dashes -> ASCII hyphen (excluding katakana long vowel U+30FC)
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
        | '\u{2212}' => '-',
        // Middle dot unification -> katakana middle dot
        '\u{00B7}' | '\u{2022}' | '\u{2219}' => '\u{30FB}',
        // Smart quotes -> ASCII
        '\u{2018}' | '\u{2019}' => '\'',
        '\u{201C}' | '\u{201D}' => '"',
        _ => ch,
    }
}

/// Applies NFKC normalization, strips decorative chars, and collapses
/// whitespace.
#[must_use]
pub fn normalize_chars(s: &str) -> String {
    let nfkc: String = s.chars().map(pre_nfkc_normalize).nfkc().collect();

    // Strip decorative characters
    let mut buf = String::with_capacity(nfkc.len());
    for ch in nfkc.chars() {
        match ch {
            '☆' | '♪' | '♥' | '♡' | '★' | '♫' | '♬' => {}
            _ => buf.push(ch),
        }
    }

    // Collapse multiple spaces
    let mut result = String::with_capacity(buf.len());
    let mut prev_space = false;
    for ch in buf.chars() {
        if ch == ' ' {
            if !prev_space {
                result.push(' ');
            }
            prev_space = true;
        } else {
            prev_space = false;
            result.push(ch);
        }
    }

    let trimmed = result.trim();
    trimmed.to_owned()
}

// ---------------------------------------------------------------------------
// NormalizeRow
// ---------------------------------------------------------------------------

/// A row in the normalize viewer table.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NormalizeRow {
    /// Syoboi title ID.
    pub tid: u32,
    /// Original Syoboi title (for display).
    pub title: String,
    /// Pre-processed title (fullwidth -> halfwidth, decorative stripped).
    pub normalized_title: String,
    /// Syoboi category value.
    pub cat: Option<u32>,
    /// First broadcast year.
    pub first_year: Option<u32>,
    /// Inferred media type from category.
    pub media_type: MediaType,
    /// Title after regex match removal (trimmed).
    pub base_query: Option<String>,
    /// Extracted season number from `SeasonNum` named group.
    pub season_num: Option<u32>,
    /// Text removed by regex match.
    pub trimmed: Option<String>,
}

// ---------------------------------------------------------------------------
// InputMode
// ---------------------------------------------------------------------------

/// Input mode for the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Normal navigation mode.
    Normal,
    /// Filter text input mode.
    Filter,
    /// Regex input mode.
    Regex,
}

/// Regex source selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegexSource {
    /// User-typed manual regex.
    Manual,
    /// Combined `regex_titles` patterns from config.
    Config,
}

// ---------------------------------------------------------------------------
// NormalizeViewerState
// ---------------------------------------------------------------------------

/// Default regex placeholder when history is empty.
const REGEX_PLACEHOLDER: &str = r"(?P<SeasonNum>\d+)期";

/// State for the normalize viewer TUI.
#[allow(clippy::module_name_repetitions)]
pub struct NormalizeViewerState {
    /// All rows.
    pub rows: Vec<NormalizeRow>,
    /// Table widget state (handles selection and scroll offset).
    pub table_state: TableState,
    /// Selected row indices (original row indices, not filtered).
    pub selected: BTreeSet<usize>,
    /// Anchor index for range selection (in filtered-index space).
    pub shift_anchor: Option<usize>,
    /// Current input mode.
    pub input_mode: InputMode,
    /// Filter text.
    pub filter: String,
    /// Cached filtered row indices.
    filtered_indices: Vec<usize>,
    /// Regex input text (manual mode).
    pub regex_input: String,
    /// Compiled regex (from last successful Enter).
    compiled_regex: Option<Regex>,
    /// Regex compilation error message.
    pub regex_error: Option<String>,
    /// Saved regex patterns (loaded from config).
    regex_history: Vec<String>,
    /// Cursor position when browsing history (None = new input).
    regex_history_cursor: Option<usize>,
    /// Draft input saved before browsing history.
    regex_draft: String,
    /// Cursor position within `regex_input` (character index, not byte).
    regex_cursor: usize,
    /// Active regex source (manual vs config).
    pub regex_source: RegexSource,
    /// Combined `regex_titles` pattern from config (joined with `|`).
    regex_titles_combined: String,
    /// Number of `regex_titles` patterns in config.
    regex_titles_count: usize,
}

impl NormalizeViewerState {
    /// Creates a new state from normalize rows, regex history, and config
    /// title patterns.
    ///
    /// Pre-fills `regex_input` with the last history entry (or the default
    /// placeholder when history is empty) and applies the regex immediately.
    #[must_use]
    pub fn new(rows: Vec<NormalizeRow>, history: Vec<String>, regex_titles: &[String]) -> Self {
        let filtered_indices: Vec<usize> = (0..rows.len()).collect();
        let mut table_state = TableState::default();
        if !rows.is_empty() {
            table_state.select(Some(0));
        }

        let prefill = history
            .last()
            .cloned()
            .unwrap_or_else(|| String::from(REGEX_PLACEHOLDER));

        let regex_titles_count = regex_titles.len();
        let regex_titles_combined = regex_titles.join("|");

        let regex_cursor = prefill.chars().count();
        let mut state = Self {
            rows,
            table_state,
            selected: BTreeSet::new(),
            shift_anchor: None,
            input_mode: InputMode::Normal,
            filter: String::new(),
            filtered_indices,
            regex_input: prefill,
            compiled_regex: None,
            regex_error: None,
            regex_history: history,
            regex_history_cursor: None,
            regex_draft: String::new(),
            regex_cursor,
            regex_source: RegexSource::Manual,
            regex_titles_combined,
            regex_titles_count,
        };
        state.apply_regex();
        state
    }

    /// Returns the cursor position in filtered-index space.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.table_state.selected().unwrap_or(0)
    }

    /// Returns filtered row indices.
    #[must_use]
    pub fn filtered_indices(&self) -> &[usize] {
        &self.filtered_indices
    }

    /// Returns the current row (under cursor) if any.
    #[must_use]
    #[allow(dead_code)]
    pub fn current_row(&self) -> Option<&NormalizeRow> {
        let idx = self.filtered_indices.get(self.cursor())?;
        self.rows.get(*idx)
    }

    /// Returns the original row index for the cursor position.
    #[must_use]
    pub fn current_original_index(&self) -> Option<usize> {
        self.filtered_indices.get(self.cursor()).copied()
    }

    /// Moves cursor up.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn move_up(&mut self) {
        let current = self.cursor();
        if current > 0 {
            self.table_state.select(Some(current - 1));
        }
    }

    /// Moves cursor down.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn move_down(&mut self) {
        let current = self.cursor();
        if current + 1 < self.filtered_indices.len() {
            self.table_state.select(Some(current + 1));
        }
    }

    /// Scrolls up by a page.
    pub fn page_up(&mut self, page_size: usize) {
        let current = self.cursor();
        self.table_state
            .select(Some(current.saturating_sub(page_size)));
    }

    /// Scrolls down by a page.
    pub fn page_down(&mut self, page_size: usize) {
        let max = self.filtered_indices.len().saturating_sub(1);
        let current = self.cursor();
        self.table_state
            .select(Some(current.saturating_add(page_size).min(max)));
    }

    /// Toggles selection of the current row and sets shift anchor.
    pub fn toggle_select(&mut self) {
        if let Some(orig_idx) = self.current_original_index() {
            if self.selected.contains(&orig_idx) {
                self.selected.remove(&orig_idx);
            } else {
                self.selected.insert(orig_idx);
            }
            self.shift_anchor = Some(self.cursor());
        }
    }

    /// Extends range selection upward from shift anchor.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn shift_move_up(&mut self) {
        let current = self.cursor();
        if current == 0 {
            return;
        }
        let anchor = self.shift_anchor.unwrap_or(current);
        self.table_state.select(Some(current - 1));
        self.apply_range_selection(anchor, current - 1);
    }

    /// Extends range selection downward from shift anchor.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn shift_move_down(&mut self) {
        let current = self.cursor();
        if current + 1 >= self.filtered_indices.len() {
            return;
        }
        let anchor = self.shift_anchor.unwrap_or(current);
        self.table_state.select(Some(current + 1));
        self.apply_range_selection(anchor, current + 1);
    }

    /// Selects all filtered-index rows between anchor and target (inclusive).
    fn apply_range_selection(&mut self, anchor: usize, target: usize) {
        let start = anchor.min(target);
        let end = anchor.max(target);
        for fi in start..=end {
            if let Some(&orig_idx) = self.filtered_indices.get(fi) {
                self.selected.insert(orig_idx);
            }
        }
    }

    /// Updates the filter and rebuilds the cache.
    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.rebuild_filter_cache();
        self.select_first();
    }

    /// Appends a character to the filter.
    pub fn filter_push(&mut self, ch: char) {
        self.filter.push(ch);
        self.rebuild_filter_cache();
        self.select_first();
    }

    /// Removes the last character from the filter.
    pub fn filter_pop(&mut self) {
        self.filter.pop();
        self.rebuild_filter_cache();
        self.select_first();
    }

    /// Selects the first row if available.
    const fn select_first(&mut self) {
        if self.filtered_indices.is_empty() {
            self.table_state.select(None);
        } else {
            self.table_state.select(Some(0));
        }
    }

    /// Rebuilds the filtered row indices cache.
    fn rebuild_filter_cache(&mut self) {
        if self.filter.is_empty() {
            self.filtered_indices = (0..self.rows.len()).collect();
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.filtered_indices = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    if r.title.to_lowercase().contains(&filter_lower) {
                        return true;
                    }
                    if let Some(ref bq) = r.base_query
                        && bq.to_lowercase().contains(&filter_lower)
                    {
                        return true;
                    }
                    r.normalized_title.to_lowercase().contains(&filter_lower)
                })
                .map(|(i, _)| i)
                .collect();
        }
    }

    /// Returns the active regex pattern based on the current source.
    fn active_pattern(&self) -> &str {
        match self.regex_source {
            RegexSource::Manual => &self.regex_input,
            RegexSource::Config => &self.regex_titles_combined,
        }
    }

    /// Compiles and applies the active regex pattern to all rows.
    ///
    /// On success, updates `base_query` and `season_num` for every row.
    /// On compile error, sets `regex_error` and leaves rows unchanged.
    /// If the pattern is empty, resets all rows.
    pub fn apply_regex(&mut self) {
        let pattern = self.active_pattern().to_owned();

        if pattern.is_empty() {
            self.compiled_regex = None;
            self.regex_error = None;
            for row in &mut self.rows {
                row.base_query = None;
                row.season_num = None;
                row.trimmed = None;
            }
            return;
        }

        match Regex::new(&pattern) {
            Ok(re) => {
                self.regex_error = None;
                for row in &mut self.rows {
                    if let Some(m) = re.find(&row.normalized_title) {
                        row.trimmed = Some(m.as_str().to_owned());
                        let mut result = String::with_capacity(row.normalized_title.len());
                        result.push_str(&row.normalized_title[..m.start()]);
                        result.push_str(&row.normalized_title[m.end()..]);
                        let base = result.trim().to_owned();
                        row.base_query = if base.is_empty() { None } else { Some(base) };
                    } else {
                        row.base_query = None;
                        row.trimmed = None;
                    }

                    // Try named group first, fall back to first digit in trimmed text.
                    row.season_num = re
                        .captures(&row.normalized_title)
                        .and_then(|caps| caps.name("SeasonNum"))
                        .and_then(|m| m.as_str().parse::<u32>().ok())
                        .or_else(|| {
                            row.trimmed.as_deref().and_then(|t| {
                                SEASON_NUM_FALLBACK
                                    .find(t)
                                    .and_then(|m| m.as_str().parse::<u32>().ok())
                            })
                        });
                }
                self.compiled_regex = Some(re);
            }
            Err(e) => {
                self.regex_error = Some(e.to_string());
            }
        }
    }

    /// Toggles between manual regex and config `regex_titles`.
    pub fn toggle_regex_source(&mut self) {
        self.regex_source = match self.regex_source {
            RegexSource::Manual => RegexSource::Config,
            RegexSource::Config => RegexSource::Manual,
        };
        self.apply_regex();
    }

    /// Returns the number of `regex_titles` patterns from config.
    #[must_use]
    pub const fn regex_titles_count(&self) -> usize {
        self.regex_titles_count
    }

    // -------------------------------------------------------------------
    // Regex cursor editing
    // -------------------------------------------------------------------

    /// Returns the byte index corresponding to the given character index.
    fn regex_byte_index(&self, char_idx: usize) -> usize {
        self.regex_input
            .char_indices()
            .nth(char_idx)
            .map_or(self.regex_input.len(), |(byte_idx, _)| byte_idx)
    }

    /// Returns the display width (terminal columns) of `regex_input` up to
    /// the current cursor position.
    #[must_use]
    pub fn regex_cursor_display_width(&self) -> usize {
        self.regex_input
            .chars()
            .take(self.regex_cursor)
            .map(|c| if c.is_ascii() { 1 } else { 2 })
            .sum()
    }

    /// Moves the regex cursor one character to the left.
    #[allow(clippy::arithmetic_side_effects)]
    pub const fn regex_cursor_left(&mut self) {
        if self.regex_cursor > 0 {
            self.regex_cursor -= 1;
        }
    }

    /// Moves the regex cursor one character to the right.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn regex_cursor_right(&mut self) {
        let len = self.regex_input.chars().count();
        if self.regex_cursor < len {
            self.regex_cursor += 1;
        }
    }

    /// Moves the regex cursor to the beginning of input.
    pub const fn regex_cursor_home(&mut self) {
        self.regex_cursor = 0;
    }

    /// Moves the regex cursor to the end of input.
    pub fn regex_cursor_end(&mut self) {
        self.regex_cursor = self.regex_input.chars().count();
    }

    /// Inserts a character at the current cursor position.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn regex_insert_char(&mut self, c: char) {
        let byte_idx = self.regex_byte_index(self.regex_cursor);
        self.regex_input.insert(byte_idx, c);
        self.regex_cursor += 1;
        self.regex_error = None;
    }

    /// Deletes the character before the cursor position.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn regex_delete_back(&mut self) {
        if self.regex_cursor > 0 {
            self.regex_cursor -= 1;
            let byte_idx = self.regex_byte_index(self.regex_cursor);
            self.regex_input.remove(byte_idx);
        }
        self.regex_error = None;
    }

    /// Resets the regex cursor to the end of the current input.
    fn regex_cursor_to_end(&mut self) {
        self.regex_cursor = self.regex_input.chars().count();
    }

    // -------------------------------------------------------------------
    // Regex history navigation
    // -------------------------------------------------------------------

    /// Navigates to the previous (older) regex history entry.
    ///
    /// On the first call, saves the current `regex_input` as a draft.
    /// Does nothing when already at the oldest entry.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn regex_history_up(&mut self) {
        if self.regex_history.is_empty() {
            return;
        }

        let new_cursor = match self.regex_history_cursor {
            None => {
                // Entering history for the first time — save draft
                self.regex_draft = self.regex_input.clone();
                self.regex_history.len() - 1
            }
            Some(0) => return, // already at oldest
            Some(c) => c - 1,
        };

        self.regex_history_cursor = Some(new_cursor);
        if let Some(pattern) = self.regex_history.get(new_cursor) {
            self.regex_input = pattern.clone();
        }
        self.regex_cursor_to_end();
    }

    /// Navigates to the next (newer) regex history entry.
    ///
    /// When moving past the newest entry, restores the saved draft.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn regex_history_down(&mut self) {
        let Some(cursor) = self.regex_history_cursor else {
            return; // not browsing history
        };

        if cursor + 1 >= self.regex_history.len() {
            // Past newest entry — restore draft
            self.regex_history_cursor = None;
            self.regex_input = self.regex_draft.clone();
        } else {
            let new_cursor = cursor + 1;
            self.regex_history_cursor = Some(new_cursor);
            if let Some(pattern) = self.regex_history.get(new_cursor) {
                self.regex_input = pattern.clone();
            }
        }
        self.regex_cursor_to_end();
    }

    /// Commits the current `regex_input` to history (deduplicating).
    ///
    /// Removes any existing occurrence of the same pattern before
    /// appending it to the end. Resets the history cursor.
    pub fn commit_regex_to_history(&mut self) {
        if self.regex_input.is_empty() {
            return;
        }
        self.regex_history.retain(|p| p != &self.regex_input);
        self.regex_history.push(self.regex_input.clone());
        self.regex_history_cursor = None;
    }

    /// Returns the current regex history (for saving back to config).
    #[must_use]
    pub fn regex_history(&self) -> &[String] {
        &self.regex_history
    }

    /// Builds TSV output lines for selected rows.
    #[must_use]
    pub fn build_output(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(String::from(
            "TID\tTitle\tBaseQuery\tTrim\tSeasonNum\tYear\tMediaType",
        ));

        for &idx in &self.selected {
            let Some(row) = self.rows.get(idx) else {
                continue;
            };

            let base_query = row.base_query.as_deref().unwrap_or(&row.normalized_title);
            let trim = row.trimmed.as_deref().unwrap_or("");

            let season = row.season_num.map_or_else(String::new, |s| s.to_string());

            let year = row.first_year.map_or_else(String::new, |y| y.to_string());

            lines.push(format!(
                "{}\t{}\t{base_query}\t{trim}\t{season}\t{year}\t{}",
                row.tid, row.title, row.media_type,
            ));
        }

        lines
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]

    use super::*;

    fn make_row(tid: u32, title: &str, cat: Option<u32>, first_year: Option<u32>) -> NormalizeRow {
        let normalized_title = normalize_chars(title);
        NormalizeRow {
            tid,
            title: title.to_owned(),
            normalized_title,
            cat,
            first_year,
            media_type: categorize(cat),
            base_query: None,
            season_num: None,
            trimmed: None,
        }
    }

    fn make_state() -> NormalizeViewerState {
        let rows = vec![
            make_row(1, "SPY×FAMILY", Some(1), Some(2023)),
            make_row(2, "Bocchi the Rock!", Some(1), Some(2022)),
            make_row(3, "進撃の巨人", Some(1), Some(2013)),
        ];
        NormalizeViewerState::new(rows, Vec::new(), &[])
    }

    // -------------------------------------------------------------------
    // categorize tests
    // -------------------------------------------------------------------

    #[test]
    fn test_categorize_tv() {
        assert_eq!(categorize(Some(1)), MediaType::Tv);
        assert_eq!(categorize(Some(3)), MediaType::Tv);
        assert_eq!(categorize(Some(4)), MediaType::Tv);
        assert_eq!(categorize(Some(10)), MediaType::Tv);
        assert_eq!(categorize(Some(0)), MediaType::Tv);
        assert_eq!(categorize(None), MediaType::Tv);
    }

    #[test]
    fn test_categorize_ova() {
        assert_eq!(categorize(Some(7)), MediaType::Ova);
    }

    #[test]
    fn test_categorize_movie() {
        assert_eq!(categorize(Some(8)), MediaType::Movie);
    }

    // -------------------------------------------------------------------
    // normalize_chars tests
    // -------------------------------------------------------------------

    #[test]
    fn test_normalize_fullwidth_to_halfwidth() {
        assert_eq!(
            normalize_chars("Ｈｅｌｌｏ　Ｗｏｒｌｄ１２３"),
            "Hello World123"
        );
    }

    #[test]
    fn test_normalize_no_change() {
        assert_eq!(normalize_chars("SPY×FAMILY"), "SPY×FAMILY");
    }

    #[test]
    fn test_normalize_trim() {
        assert_eq!(normalize_chars("  hello  "), "hello");
    }

    #[test]
    fn test_normalize_decorative_stripped() {
        assert_eq!(normalize_chars("プリパラ☆ミ"), "プリパラミ");
    }

    #[test]
    fn test_normalize_multi_decorative() {
        assert_eq!(
            normalize_chars("アイドル♪マスター♡シンデレラ"),
            "アイドルマスターシンデレラ"
        );
    }

    #[test]
    fn test_normalize_multi_space_collapsed() {
        assert_eq!(normalize_chars("A  B   C"), "A B C");
    }

    #[test]
    fn test_normalize_fullwidth_space() {
        assert_eq!(normalize_chars("A\u{3000}B"), "A B");
    }

    // -------------------------------------------------------------------
    // apply_regex tests
    // -------------------------------------------------------------------

    #[test]
    fn test_apply_regex_season_jp() {
        // Arrange
        let rows = vec![make_row(
            5656,
            "はたらく魔王さま!! 第2期",
            Some(1),
            Some(2022),
        )];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &[]);
        state.regex_input = String::from(r"第(?P<SeasonNum>\d+)期");

        // Act
        state.apply_regex();

        // Assert
        assert!(state.regex_error.is_none());
        assert_eq!(
            state.rows[0].base_query.as_deref(),
            Some("はたらく魔王さま!!")
        );
        assert_eq!(state.rows[0].season_num, Some(2));
        assert_eq!(state.rows[0].trimmed.as_deref(), Some("第2期"));
    }

    #[test]
    fn test_apply_regex_season_en() {
        // Arrange
        let rows = vec![make_row(6668, "SPY×FAMILY Season 2", Some(1), Some(2023))];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &[]);
        state.regex_input = String::from(r"Season\s+(?P<SeasonNum>\d+)");

        // Act
        state.apply_regex();

        // Assert
        assert!(state.regex_error.is_none());
        assert_eq!(state.rows[0].base_query.as_deref(), Some("SPY×FAMILY"));
        assert_eq!(state.rows[0].season_num, Some(2));
        assert_eq!(state.rows[0].trimmed.as_deref(), Some("Season 2"));
    }

    #[test]
    fn test_apply_regex_no_match() {
        // Arrange
        let rows = vec![make_row(6750, "葬送のフリーレン", Some(1), Some(2023))];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &[]);
        state.regex_input = String::from(r"第(?P<SeasonNum>\d+)期");

        // Act
        state.apply_regex();

        // Assert
        assert!(state.regex_error.is_none());
        assert!(state.rows[0].base_query.is_none());
        assert!(state.rows[0].season_num.is_none());
        assert!(state.rows[0].trimmed.is_none());
    }

    #[test]
    fn test_apply_regex_invalid_pattern() {
        // Arrange
        let mut state = make_state();
        state.regex_input = String::from(r"(unclosed");

        // Act
        state.apply_regex();

        // Assert
        assert!(state.regex_error.is_some());
        // Rows should remain unchanged
        assert!(state.rows[0].base_query.is_none());
    }

    #[test]
    fn test_apply_regex_empty_resets() {
        // Arrange
        let rows = vec![make_row(
            5656,
            "はたらく魔王さま!! 第2期",
            Some(1),
            Some(2022),
        )];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &[]);

        // First apply a regex
        state.regex_input = String::from(r"第(?P<SeasonNum>\d+)期");
        state.apply_regex();
        assert!(state.rows[0].base_query.is_some());

        // Now reset with empty
        state.regex_input = String::new();
        state.apply_regex();

        // Assert
        assert!(state.rows[0].base_query.is_none());
        assert!(state.rows[0].season_num.is_none());
        assert!(state.rows[0].trimmed.is_none());
        assert!(state.regex_error.is_none());
    }

    #[test]
    fn test_apply_regex_no_season_group_fallback() {
        // Arrange: regex without SeasonNum named group — fallback extracts from trimmed
        let rows = vec![make_row(
            5656,
            "はたらく魔王さま!! 第2期",
            Some(1),
            Some(2022),
        )];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &[]);
        state.regex_input = String::from(r"第\d+期");

        // Act
        state.apply_regex();

        // Assert
        assert!(state.regex_error.is_none());
        assert_eq!(
            state.rows[0].base_query.as_deref(),
            Some("はたらく魔王さま!!")
        );
        assert_eq!(state.rows[0].season_num, Some(2));
    }

    // -------------------------------------------------------------------
    // Navigation tests
    // -------------------------------------------------------------------

    #[test]
    fn test_initial_state() {
        // Arrange & Act
        let state = make_state();

        // Assert
        assert_eq!(state.filtered_indices().len(), 3);
        assert_eq!(state.cursor(), 0);
        assert!(state.selected.is_empty());
    }

    #[test]
    fn test_move_down_and_up() {
        // Arrange
        let mut state = make_state();

        // Act & Assert
        state.move_down();
        assert_eq!(state.cursor(), 1);

        state.move_down();
        assert_eq!(state.cursor(), 2);

        state.move_down(); // at end
        assert_eq!(state.cursor(), 2);

        state.move_up();
        assert_eq!(state.cursor(), 1);

        state.move_up();
        assert_eq!(state.cursor(), 0);

        state.move_up(); // at start
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn test_page_up_down() {
        // Arrange
        let mut state = make_state();

        // Act
        state.page_down(10);

        // Assert: clamped to last item
        assert_eq!(state.cursor(), 2);

        // Act
        state.page_up(10);

        // Assert
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn test_toggle_select() {
        // Arrange
        let mut state = make_state();

        // Act: select first row
        state.toggle_select();

        // Assert
        assert!(state.selected.contains(&0));
        assert_eq!(state.shift_anchor, Some(0));

        // Act: toggle off
        state.toggle_select();

        // Assert
        assert!(!state.selected.contains(&0));
    }

    #[test]
    fn test_shift_move_down() {
        // Arrange
        let mut state = make_state();
        state.toggle_select(); // select row 0, anchor=0

        // Act: shift+down twice
        state.shift_move_down();
        state.shift_move_down();

        // Assert: rows 0,1,2 selected
        assert!(state.selected.contains(&0));
        assert!(state.selected.contains(&1));
        assert!(state.selected.contains(&2));
        assert_eq!(state.cursor(), 2);
    }

    #[test]
    fn test_shift_move_up() {
        // Arrange
        let mut state = make_state();
        state.move_down();
        state.move_down(); // cursor at 2
        state.toggle_select(); // anchor=2

        // Act
        state.shift_move_up();

        // Assert: rows 1,2 selected
        assert!(state.selected.contains(&1));
        assert!(state.selected.contains(&2));
        assert_eq!(state.cursor(), 1);
    }

    // -------------------------------------------------------------------
    // Filter tests
    // -------------------------------------------------------------------

    #[test]
    fn test_filter() {
        // Arrange
        let mut state = make_state();

        // Act
        state.set_filter(String::from("spy"));

        // Assert
        assert_eq!(state.filtered_indices().len(), 1);
        assert_eq!(state.current_row().unwrap().tid, 1);
    }

    #[test]
    fn test_filter_no_match() {
        // Arrange
        let mut state = make_state();

        // Act
        state.set_filter(String::from("nonexistent"));

        // Assert
        assert!(state.filtered_indices().is_empty());
        assert!(state.current_row().is_none());
    }

    // -------------------------------------------------------------------
    // TSV output tests
    // -------------------------------------------------------------------

    #[test]
    fn test_build_output_empty() {
        // Arrange
        let state = make_state();

        // Act
        let output = state.build_output();

        // Assert: header only
        assert_eq!(output.len(), 1);
        assert!(output[0].starts_with("TID\t"));
    }

    #[test]
    fn test_build_output_with_selection() {
        // Arrange
        let mut state = make_state();
        state.toggle_select(); // select row 0

        // Act
        let output = state.build_output();

        // Assert
        assert_eq!(output.len(), 2);
        assert!(output[1].starts_with("1\t"));
        assert!(output[1].contains("SPY×FAMILY"));
    }

    #[test]
    fn test_build_output_with_regex_applied() {
        // Arrange
        let rows = vec![make_row(
            5656,
            "はたらく魔王さま!! 第2期",
            Some(1),
            Some(2022),
        )];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &[]);
        state.regex_input = String::from(r"第(?P<SeasonNum>\d+)期");
        state.apply_regex();
        state.toggle_select();

        // Act
        let output = state.build_output();

        // Assert
        assert_eq!(output.len(), 2);
        assert!(output[1].contains("はたらく魔王さま!!"));
        assert!(output[1].contains("\t第2期\t"));
        assert!(output[1].contains("\t2\t"));
    }

    // -------------------------------------------------------------------
    // Regex history tests
    // -------------------------------------------------------------------

    #[test]
    fn test_new_with_history_prefills_last() {
        // Arrange
        let rows = vec![make_row(
            5656,
            "はたらく魔王さま!! 第2期",
            Some(1),
            Some(2022),
        )];
        let history = vec![
            String::from(r"Season\s+(?P<SeasonNum>\d+)"),
            String::from(r"第(?P<SeasonNum>\d+)期"),
        ];

        // Act
        let state = NormalizeViewerState::new(rows, history, &[]);

        // Assert — last history entry is pre-filled and applied
        assert_eq!(state.regex_input, r"第(?P<SeasonNum>\d+)期");
        assert!(state.regex_error.is_none());
        assert_eq!(
            state.rows[0].base_query.as_deref(),
            Some("はたらく魔王さま!!")
        );
        assert_eq!(state.rows[0].season_num, Some(2));
    }

    #[test]
    fn test_new_without_history_prefills_placeholder() {
        // Arrange
        let rows = vec![make_row(
            5656,
            "はたらく魔王さま!! 第2期",
            Some(1),
            Some(2022),
        )];

        // Act
        let state = NormalizeViewerState::new(rows, Vec::new(), &[]);

        // Assert — placeholder is pre-filled
        assert_eq!(state.regex_input, REGEX_PLACEHOLDER);
        assert!(state.regex_error.is_none());
    }

    #[test]
    fn test_regex_history_up_down() {
        // Arrange
        let rows = vec![make_row(1, "Test", Some(1), None)];
        let history = vec![
            String::from("pattern_a"),
            String::from("pattern_b"),
            String::from("pattern_c"),
        ];
        let mut state = NormalizeViewerState::new(rows, history, &[]);
        // regex_input is pre-filled with "pattern_c" (last entry)
        state.regex_input = String::from("current_input");

        // Act: go up once — should save draft and go to last entry
        state.regex_history_up();

        // Assert
        assert_eq!(state.regex_draft, "current_input");
        assert_eq!(state.regex_input, "pattern_c");
        assert_eq!(state.regex_history_cursor, Some(2));

        // Act: go up again
        state.regex_history_up();
        assert_eq!(state.regex_input, "pattern_b");
        assert_eq!(state.regex_history_cursor, Some(1));

        // Act: go up again
        state.regex_history_up();
        assert_eq!(state.regex_input, "pattern_a");
        assert_eq!(state.regex_history_cursor, Some(0));

        // Act: go up at oldest — no change
        state.regex_history_up();
        assert_eq!(state.regex_input, "pattern_a");
        assert_eq!(state.regex_history_cursor, Some(0));

        // Act: go down
        state.regex_history_down();
        assert_eq!(state.regex_input, "pattern_b");
        assert_eq!(state.regex_history_cursor, Some(1));

        // Act: go down
        state.regex_history_down();
        assert_eq!(state.regex_input, "pattern_c");
        assert_eq!(state.regex_history_cursor, Some(2));

        // Act: go down past newest — restore draft
        state.regex_history_down();
        assert_eq!(state.regex_input, "current_input");
        assert!(state.regex_history_cursor.is_none());

        // Act: go down again — no effect
        state.regex_history_down();
        assert_eq!(state.regex_input, "current_input");
    }

    #[test]
    fn test_commit_regex_to_history() {
        // Arrange
        let rows = vec![make_row(1, "Test", Some(1), None)];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &[]);
        state.regex_input = String::from("new_pattern");

        // Act
        state.commit_regex_to_history();

        // Assert
        assert_eq!(state.regex_history(), &["new_pattern"]);
    }

    #[test]
    fn test_commit_regex_deduplicates() {
        // Arrange
        let rows = vec![make_row(1, "Test", Some(1), None)];
        let history = vec![String::from("pattern_a"), String::from("pattern_b")];
        let mut state = NormalizeViewerState::new(rows, history, &[]);

        // Act: commit an existing pattern
        state.regex_input = String::from("pattern_a");
        state.commit_regex_to_history();

        // Assert: moved to end, no duplicate
        assert_eq!(state.regex_history(), &["pattern_b", "pattern_a"]);
    }

    #[test]
    fn test_commit_regex_empty_is_noop() {
        // Arrange
        let rows = vec![make_row(1, "Test", Some(1), None)];
        let history = vec![String::from("existing")];
        let mut state = NormalizeViewerState::new(rows, history, &[]);
        state.regex_input = String::new();

        // Act
        state.commit_regex_to_history();

        // Assert — history unchanged
        assert_eq!(state.regex_history(), &["existing"]);
    }

    #[test]
    fn test_regex_history_up_empty_history() {
        // Arrange
        let rows = vec![make_row(1, "Test", Some(1), None)];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &[]);

        // Act — should be a no-op
        state.regex_history_up();

        // Assert
        assert_eq!(state.regex_input, REGEX_PLACEHOLDER);
        assert!(state.regex_history_cursor.is_none());
    }

    // -------------------------------------------------------------------
    // NFKC normalization tests
    // -------------------------------------------------------------------

    #[test]
    fn test_normalize_wave_dash() {
        // U+301C wave dash -> ~
        assert_eq!(normalize_chars("A\u{301C}B"), "A~B");
    }

    #[test]
    fn test_normalize_fullwidth_tilde() {
        // U+FF5E fullwidth tilde -> ~ (via NFKC)
        assert_eq!(normalize_chars("A\u{FF5E}B"), "A~B");
    }

    #[test]
    fn test_normalize_wave_dash_and_fullwidth_tilde_unify() {
        // Both wave dash and fullwidth tilde normalize to ~
        let a = normalize_chars("A\u{301C}B");
        let b = normalize_chars("A\u{FF5E}B");
        assert_eq!(a, b);
        assert_eq!(a, "A~B");
    }

    #[test]
    fn test_normalize_halfwidth_katakana() {
        assert_eq!(normalize_chars("ｶﾀｶﾅ"), "カタカナ");
    }

    #[test]
    fn test_normalize_halfwidth_katakana_with_dakuten() {
        assert_eq!(normalize_chars("ｶﾞ"), "ガ");
    }

    #[test]
    fn test_normalize_roman_numerals() {
        assert_eq!(normalize_chars("Ⅳ"), "IV");
        assert_eq!(normalize_chars("Ⅱ"), "II");
    }

    #[test]
    fn test_normalize_circled_numbers() {
        assert_eq!(normalize_chars("①"), "1");
    }

    #[test]
    fn test_normalize_various_dashes() {
        // U+2010 hyphen, U+2013 en dash, U+2014 em dash,
        // U+2015 horizontal bar, U+2212 minus sign -> '-'
        assert_eq!(normalize_chars("A\u{2010}B"), "A-B");
        assert_eq!(normalize_chars("A\u{2013}B"), "A-B");
        assert_eq!(normalize_chars("A\u{2014}B"), "A-B");
        assert_eq!(normalize_chars("A\u{2015}B"), "A-B");
        assert_eq!(normalize_chars("A\u{2212}B"), "A-B");
    }

    #[test]
    fn test_normalize_long_vowel_preserved() {
        // U+30FC katakana long vowel mark should NOT be converted
        assert_eq!(normalize_chars("ラーメン"), "ラーメン");
    }

    #[test]
    fn test_normalize_smart_quotes() {
        assert_eq!(normalize_chars("\u{201C}Hello\u{201D}"), "\"Hello\"");
        assert_eq!(normalize_chars("\u{2018}world\u{2019}"), "'world'");
    }

    #[test]
    fn test_normalize_middle_dot_unification() {
        // U+00B7 middle dot, U+2022 bullet, U+2219 bullet operator
        // all -> U+30FB katakana middle dot
        assert_eq!(normalize_chars("A\u{00B7}B"), "A\u{30FB}B");
        assert_eq!(normalize_chars("A\u{2022}B"), "A\u{30FB}B");
        assert_eq!(normalize_chars("A\u{2219}B"), "A\u{30FB}B");
    }

    #[test]
    fn test_normalize_fullwidth_exclamation() {
        assert_eq!(normalize_chars("！！"), "!!");
    }

    // -------------------------------------------------------------------
    // RegexSource toggle tests
    // -------------------------------------------------------------------

    #[test]
    fn test_season_num_fallback_config_regex_japanese() {
        // Arrange: config regex without named group — fallback extracts 2 from "(第2期)"
        let rows = vec![make_row(1, "ダンダダン(第2期)", Some(1), Some(2024))];
        let regex_titles = vec![String::from(r"\s*\(第\d+(?:期|クール)\)")];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &regex_titles);

        // Act
        state.toggle_regex_source();

        // Assert
        assert_eq!(state.regex_source, RegexSource::Config);
        assert_eq!(state.rows[0].base_query.as_deref(), Some("ダンダダン"));
        assert_eq!(state.rows[0].season_num, Some(2));
    }

    #[test]
    fn test_season_num_fallback_config_regex_english() {
        // Arrange: config regex without named group — fallback extracts 3 from " Season 3"
        let rows = vec![make_row(1, "SPY×FAMILY Season 3", Some(1), Some(2023))];
        let regex_titles = vec![String::from(r"(?i:\s*season\s*\d+)")];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &regex_titles);

        // Act
        state.toggle_regex_source();

        // Assert
        assert_eq!(state.rows[0].base_query.as_deref(), Some("SPY×FAMILY"));
        assert_eq!(state.rows[0].season_num, Some(3));
    }

    #[test]
    fn test_season_num_fallback_no_digits() {
        // Arrange: "FINAL SEASON" has no digits → season_num should be None
        let rows = vec![make_row(1, "進撃の巨人 FINAL SEASON", Some(1), Some(2023))];
        let regex_titles = vec![String::from(r"(?i:\s*(?:FINAL\s+)?SEASON(?:\s*\d+)?)")];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &regex_titles);

        // Act
        state.toggle_regex_source();

        // Assert
        assert_eq!(state.rows[0].base_query.as_deref(), Some("進撃の巨人"));
        assert!(state.rows[0].season_num.is_none());
    }

    #[test]
    fn test_season_num_named_group_takes_precedence() {
        // Arrange: regex WITH named group — should use named group, not fallback
        let rows = vec![make_row(1, "タイトル 第5期", Some(1), Some(2024))];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &[]);
        state.regex_input = String::from(r"\s*第(?P<SeasonNum>\d+)期");

        // Act
        state.apply_regex();

        // Assert
        assert_eq!(state.rows[0].season_num, Some(5));
    }

    #[test]
    fn test_toggle_regex_source_applies_config() {
        // Arrange
        let rows = vec![
            make_row(1, "SPY×FAMILY Season 3", Some(1), Some(2023)),
            make_row(2, "ダンダダン(第2期)", Some(1), Some(2024)),
            make_row(3, "葬送のフリーレン", Some(1), Some(2023)),
        ];
        let regex_titles = vec![
            String::from(r"\s*\(第\d+(?:期|クール)\)"),
            String::from(r"(?i:\s*season\s*\d+)"),
        ];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &regex_titles);

        // Act
        state.toggle_regex_source();

        // Assert
        assert_eq!(state.regex_source, RegexSource::Config);
        assert!(state.regex_error.is_none());
        assert_eq!(state.rows[0].base_query.as_deref(), Some("SPY×FAMILY"));
        assert_eq!(state.rows[0].season_num, Some(3));
        assert_eq!(state.rows[1].base_query.as_deref(), Some("ダンダダン"));
        assert_eq!(state.rows[1].season_num, Some(2));
        assert!(state.rows[2].base_query.is_none());
        assert!(state.rows[2].season_num.is_none());
    }

    #[test]
    fn test_toggle_regex_source_back_to_manual() {
        // Arrange
        let rows = vec![make_row(1, "SPY×FAMILY Season 3", Some(1), Some(2023))];
        let regex_titles = vec![String::from(r"(?i:\s*season\s*\d+)")];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &regex_titles);
        state.regex_input = String::from(r"SPY");

        // Act: toggle to config, then back to manual
        state.toggle_regex_source();
        assert_eq!(state.regex_source, RegexSource::Config);
        state.toggle_regex_source();

        // Assert: manual regex is re-applied
        assert_eq!(state.regex_source, RegexSource::Manual);
        assert_eq!(
            state.rows[0].base_query.as_deref(),
            Some("×FAMILY Season 3")
        );
    }

    #[test]
    fn test_toggle_config_empty_patterns() {
        // Arrange
        let rows = vec![make_row(1, "SPY×FAMILY Season 3", Some(1), Some(2023))];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &[]);

        // Act
        state.toggle_regex_source();

        // Assert: empty config resets rows
        assert_eq!(state.regex_source, RegexSource::Config);
        assert!(state.rows[0].base_query.is_none());
        assert!(state.rows[0].trimmed.is_none());
    }

    #[test]
    fn test_regex_titles_count() {
        // Arrange
        let rows = vec![make_row(1, "Test", Some(1), None)];
        let regex_titles = vec![String::from("a"), String::from("b"), String::from("c")];
        let state = NormalizeViewerState::new(rows, Vec::new(), &regex_titles);

        // Assert
        assert_eq!(state.regex_titles_count(), 3);
    }

    #[test]
    fn test_initial_regex_source_is_manual() {
        // Arrange & Act
        let state = make_state();

        // Assert
        assert_eq!(state.regex_source, RegexSource::Manual);
    }

    #[test]
    fn test_regex_cursor_home_end() {
        // Arrange: initial state has REGEX_PLACEHOLDER
        let mut state = make_state();
        let initial_len = state.regex_input.chars().count();
        assert_eq!(state.regex_cursor, initial_len);

        // Act: Home
        state.regex_cursor_home();

        // Assert
        assert_eq!(state.regex_cursor, 0);

        // Act: End
        state.regex_cursor_end();
        assert_eq!(state.regex_cursor, initial_len);
    }

    #[test]
    fn test_regex_delete_back_at_start() {
        // Arrange
        let mut state = make_state();
        state.regex_cursor_home();
        let original_input = state.regex_input.clone();
        assert_eq!(state.regex_cursor, 0);

        // Act: delete at position 0 is no-op
        state.regex_delete_back();

        // Assert
        assert_eq!(state.regex_cursor, 0);
        assert_eq!(state.regex_input, original_input);
    }

    #[test]
    fn test_toggle_regex_source_back_to_manual_preserves_input() {
        // Arrange
        let rows = vec![make_row(1, "タイトル 第2期", Some(1), None)];
        let regex_titles = vec![String::from(r"第\d+期$")];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &regex_titles);
        state.regex_insert_char('x');

        // Act: Manual -> Config -> Manual
        state.toggle_regex_source();
        assert_eq!(state.regex_source, RegexSource::Config);
        state.toggle_regex_source();

        // Assert: back to Manual, input preserved
        assert_eq!(state.regex_source, RegexSource::Manual);
    }

    #[test]
    fn test_build_output_with_selections() {
        // Arrange
        let mut state = make_state();
        state.toggle_select(); // select row 0

        // Act
        let output = state.build_output();

        // Assert: header + 1 data line
        assert_eq!(output.len(), 2);
        assert!(output[0].starts_with("TID\t"));
        assert!(output[1].starts_with("1\t"));
    }

    // -------------------------------------------------------------------
    // MediaType Display tests
    // -------------------------------------------------------------------

    #[test]
    fn test_media_type_display() {
        assert_eq!(MediaType::Movie.to_string(), "Movie");
        assert_eq!(MediaType::Ova.to_string(), "Ova");
        assert_eq!(MediaType::Tv.to_string(), "Tv");
    }

    // -------------------------------------------------------------------
    // Filter push/pop tests
    // -------------------------------------------------------------------

    #[test]
    fn test_filter_push_pop() {
        // Arrange
        let mut state = make_state();
        assert_eq!(state.filtered_indices().len(), 3);

        // Act: type "spy" char by char
        state.filter_push('s');
        state.filter_push('p');
        state.filter_push('y');

        // Assert: only SPY×FAMILY visible
        assert_eq!(state.filtered_indices().len(), 1);
        assert_eq!(state.current_row().unwrap().tid, 1);
        assert_eq!(state.filter, "spy");

        // Act: pop all
        state.filter_pop();
        state.filter_pop();
        state.filter_pop();

        // Assert: all rows visible
        assert!(state.filter.is_empty());
        assert_eq!(state.filtered_indices().len(), 3);
    }

    #[test]
    fn test_filter_by_base_query() {
        // Arrange: apply regex so base_query is set, then filter by it
        let rows = vec![
            make_row(1, "SPY×FAMILY Season 2", Some(1), Some(2023)),
            make_row(2, "Bocchi the Rock!", Some(1), Some(2022)),
        ];
        let mut state = NormalizeViewerState::new(rows, Vec::new(), &[]);
        state.regex_input = String::from(r"Season\s+\d+");
        state.apply_regex();
        // base_query for row 0 is "SPY×FAMILY"

        // Act: filter by "SPY"
        state.set_filter(String::from("SPY"));

        // Assert: both title and base_query match "SPY" for row 0
        assert_eq!(state.filtered_indices().len(), 1);
        assert_eq!(state.current_row().unwrap().tid, 1);
    }

    // -------------------------------------------------------------------
    // Regex cursor left/right tests
    // -------------------------------------------------------------------

    #[test]
    fn test_regex_cursor_left_right() {
        // Arrange
        let mut state = make_state();
        state.regex_input = String::from("abc");
        state.regex_cursor = 3;

        // Act: move left
        state.regex_cursor_left();
        assert_eq!(state.regex_cursor, 2);

        state.regex_cursor_left();
        assert_eq!(state.regex_cursor, 1);

        state.regex_cursor_left();
        assert_eq!(state.regex_cursor, 0);

        // At 0, should not move
        state.regex_cursor_left();
        assert_eq!(state.regex_cursor, 0);

        // Move right
        state.regex_cursor_right();
        assert_eq!(state.regex_cursor, 1);

        state.regex_cursor_right();
        state.regex_cursor_right();
        assert_eq!(state.regex_cursor, 3);

        // At end, should not move
        state.regex_cursor_right();
        assert_eq!(state.regex_cursor, 3);
    }

    #[test]
    fn test_regex_cursor_display_width() {
        // Arrange: mix of ASCII and multibyte
        let mut state = make_state();
        state.regex_input = String::from("aあb");
        state.regex_cursor = 2; // "aあ" = 1 + 2 = 3 columns

        // Act
        let width = state.regex_cursor_display_width();

        // Assert
        assert_eq!(width, 3);
    }

    // -------------------------------------------------------------------
    // Shift move boundary tests
    // -------------------------------------------------------------------

    #[test]
    fn test_shift_move_up_at_zero() {
        // Arrange: cursor at 0
        let mut state = make_state();
        assert_eq!(state.cursor(), 0);

        // Act: shift_move_up at 0 is no-op
        state.shift_move_up();

        // Assert
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn test_shift_move_down_at_end() {
        // Arrange: cursor at last row
        let mut state = make_state();
        state.move_down();
        state.move_down(); // cursor at 2 (last)
        assert_eq!(state.cursor(), 2);

        // Act: shift_move_down at end is no-op
        state.shift_move_down();

        // Assert
        assert_eq!(state.cursor(), 2);
    }

    // -------------------------------------------------------------------
    // Regex delete_back inner path
    // -------------------------------------------------------------------

    #[test]
    fn test_regex_delete_back_inner() {
        // Arrange: cursor in the middle of input
        let mut state = make_state();
        state.regex_input = String::from("abcde");
        state.regex_cursor = 3; // after 'c'

        // Act
        state.regex_delete_back();

        // Assert: 'c' removed, cursor moves back
        assert_eq!(state.regex_input, "abde");
        assert_eq!(state.regex_cursor, 2);
    }
}
