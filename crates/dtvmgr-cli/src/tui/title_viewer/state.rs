//! Title viewer TUI state management.

use std::collections::HashMap;

use ratatui::widgets::TableState;

/// A title row for display.
#[derive(Debug, Clone)]
pub struct TitleRow {
    /// Syoboi title ID.
    pub tid: u32,
    /// Title name.
    pub title: String,
    /// First broadcast year.
    pub first_year: Option<u32>,
    /// TMDB series ID (if mapped).
    pub tmdb_series_id: Option<u64>,
    /// TMDB season number (if mapped).
    pub tmdb_season_number: Option<u32>,
    /// Number of programs for this title.
    pub program_count: usize,
}

/// A program row for display.
#[derive(Debug, Clone)]
pub struct ProgramRow {
    /// Program ID.
    pub pid: u32,
    /// Episode number.
    pub count: Option<u32>,
    /// Broadcast start time.
    pub st_time: String,
    /// Channel name.
    pub ch_name: String,
    /// Flag bitmask (nullable).
    pub flag: Option<u32>,
    /// Duration in minutes (nullable).
    pub duration_min: Option<u32>,
    /// Episode subtitle.
    pub sub_title: Option<String>,
}

/// Currently focused pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    /// Title list pane (left).
    Titles,
    /// Program list pane (right).
    Programs,
}

/// Input mode for the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Normal navigation mode.
    Normal,
    /// Filter text input mode.
    Filter,
}

/// Summary statistics for the DB viewer header.
#[derive(Debug, Clone)]
pub struct ViewerStats {
    /// Total number of titles.
    pub total_titles: usize,
    /// Total number of programs.
    pub total_programs: usize,
    /// Number of unique channels with at least one program.
    pub unique_channels: usize,
    /// Earliest program start time (if any).
    pub oldest_st_time: Option<String>,
    /// Latest program start time (if any).
    pub newest_st_time: Option<String>,
}

/// State for the title viewer TUI.
#[allow(clippy::module_name_repetitions)]
pub struct TitleViewerState {
    /// All title rows.
    pub titles: Vec<TitleRow>,
    /// Programs grouped by TID.
    pub programs_by_tid: HashMap<u32, Vec<ProgramRow>>,
    /// Summary statistics.
    pub stats: ViewerStats,
    /// Currently focused pane.
    pub active_pane: ActivePane,
    /// Table state for the title list (handles selection and scroll).
    pub title_table_state: TableState,
    /// Table state for the program list (handles selection and scroll).
    pub program_table_state: TableState,
    /// Current input mode.
    pub input_mode: InputMode,
    /// Filter text.
    pub filter: String,
    /// Cached filtered title indices.
    filtered_indices: Vec<usize>,
}

impl TitleViewerState {
    /// Creates a new state from title and program data.
    #[must_use]
    pub fn new(
        titles: Vec<TitleRow>,
        programs_by_tid: HashMap<u32, Vec<ProgramRow>>,
        stats: ViewerStats,
    ) -> Self {
        let filtered_indices: Vec<usize> = (0..titles.len()).collect();
        let mut title_table_state = TableState::default();
        if !titles.is_empty() {
            title_table_state.select(Some(0));
        }
        Self {
            titles,
            programs_by_tid,
            stats,
            active_pane: ActivePane::Titles,
            title_table_state,
            program_table_state: TableState::default(),
            input_mode: InputMode::Normal,
            filter: String::new(),
            filtered_indices,
        }
    }

    /// Returns the title cursor position.
    #[must_use]
    pub fn title_cursor(&self) -> usize {
        self.title_table_state.selected().unwrap_or(0)
    }

    /// Returns the program cursor position.
    #[must_use]
    pub fn program_cursor(&self) -> usize {
        self.program_table_state.selected().unwrap_or(0)
    }

    /// Returns filtered title indices.
    #[must_use]
    pub fn filtered_titles(&self) -> &[usize] {
        &self.filtered_indices
    }

    /// Returns the current title row (if any).
    #[must_use]
    pub fn current_title(&self) -> Option<&TitleRow> {
        let idx = self.filtered_indices.get(self.title_cursor())?;
        self.titles.get(*idx)
    }

    /// Returns programs for the current title.
    #[must_use]
    pub fn current_programs(&self) -> &[ProgramRow] {
        self.current_title()
            .and_then(|t| self.programs_by_tid.get(&t.tid))
            .map_or(&[], Vec::as_slice)
    }

    /// Moves cursor up.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn move_up(&mut self) {
        match self.active_pane {
            ActivePane::Titles => {
                let current = self.title_cursor();
                if current > 0 {
                    self.title_table_state.select(Some(current - 1));
                }
            }
            ActivePane::Programs => {
                let current = self.program_cursor();
                if current > 0 {
                    self.program_table_state.select(Some(current - 1));
                }
            }
        }
    }

    /// Moves cursor down.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn move_down(&mut self) {
        match self.active_pane {
            ActivePane::Titles => {
                let current = self.title_cursor();
                if current + 1 < self.filtered_indices.len() {
                    self.title_table_state.select(Some(current + 1));
                }
            }
            ActivePane::Programs => {
                let count = self.current_programs().len();
                let current = self.program_cursor();
                if current + 1 < count {
                    self.program_table_state.select(Some(current + 1));
                }
            }
        }
    }

    /// Scrolls up by a page.
    pub fn page_up(&mut self, page_size: usize) {
        match self.active_pane {
            ActivePane::Titles => {
                let current = self.title_cursor();
                self.title_table_state
                    .select(Some(current.saturating_sub(page_size)));
            }
            ActivePane::Programs => {
                let current = self.program_cursor();
                self.program_table_state
                    .select(Some(current.saturating_sub(page_size)));
            }
        }
    }

    /// Scrolls down by a page.
    pub fn page_down(&mut self, page_size: usize) {
        match self.active_pane {
            ActivePane::Titles => {
                let max = self.filtered_indices.len().saturating_sub(1);
                let current = self.title_cursor();
                self.title_table_state
                    .select(Some(current.saturating_add(page_size).min(max)));
            }
            ActivePane::Programs => {
                let max = self.current_programs().len().saturating_sub(1);
                let current = self.program_cursor();
                self.program_table_state
                    .select(Some(current.saturating_add(page_size).min(max)));
            }
        }
    }

    /// Focuses the programs pane (right).
    pub fn focus_programs(&mut self) {
        self.active_pane = ActivePane::Programs;
        if self.program_table_state.selected().is_none() {
            self.program_table_state.select(Some(0));
        }
    }

    /// Focuses the titles pane (left).
    pub const fn focus_titles(&mut self) {
        self.active_pane = ActivePane::Titles;
    }

    /// Updates the filter and rebuilds the cache.
    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.rebuild_filter_cache();
        self.select_first_title();
    }

    /// Appends a character to the filter.
    pub fn filter_push(&mut self, ch: char) {
        self.filter.push(ch);
        self.rebuild_filter_cache();
        self.select_first_title();
    }

    /// Removes the last character from the filter.
    pub fn filter_pop(&mut self) {
        self.filter.pop();
        self.rebuild_filter_cache();
        self.select_first_title();
    }

    /// Selects the first title if available.
    fn select_first_title(&mut self) {
        if self.filtered_indices.is_empty() {
            self.title_table_state.select(None);
        } else {
            self.title_table_state.select(Some(0));
        }
    }

    /// Rebuilds the filtered title indices cache.
    fn rebuild_filter_cache(&mut self) {
        if self.filter.is_empty() {
            self.filtered_indices = (0..self.titles.len()).collect();
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.filtered_indices = self
                .titles
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    // Match title name
                    if t.title.to_lowercase().contains(&filter_lower) {
                        return true;
                    }
                    // Match program sub_title or st_time
                    self.programs_by_tid.get(&t.tid).is_some_and(|progs| {
                        progs.iter().any(|p| {
                            p.st_time.contains(&filter_lower)
                                || p.sub_title
                                    .as_ref()
                                    .is_some_and(|s| s.to_lowercase().contains(&filter_lower))
                        })
                    })
                })
                .map(|(i, _)| i)
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn make_state() -> TitleViewerState {
        let titles = vec![
            TitleRow {
                tid: 1,
                title: String::from("SPY×FAMILY"),
                first_year: Some(2022),
                tmdb_series_id: Some(12345),
                tmdb_season_number: Some(1),
                program_count: 2,
            },
            TitleRow {
                tid: 2,
                title: String::from("Bocchi the Rock!"),
                first_year: Some(2022),
                tmdb_series_id: None,
                tmdb_season_number: None,
                program_count: 1,
            },
        ];

        let mut programs_by_tid = HashMap::new();
        programs_by_tid.insert(
            1,
            vec![
                ProgramRow {
                    pid: 100,
                    count: Some(1),
                    st_time: String::from("2022-04-09 23:00:00"),
                    ch_name: String::from("テレビ東京"),
                    flag: None,
                    duration_min: Some(30),
                    sub_title: Some(String::from("オペレーション〈梟〉")),
                },
                ProgramRow {
                    pid: 101,
                    count: Some(2),
                    st_time: String::from("2022-04-16 23:00:00"),
                    ch_name: String::from("テレビ東京"),
                    flag: Some(2),
                    duration_min: Some(30),
                    sub_title: Some(String::from("妻役を確保せよ")),
                },
            ],
        );
        programs_by_tid.insert(
            2,
            vec![ProgramRow {
                pid: 200,
                count: Some(1),
                st_time: String::from("2022-10-08 23:30:00"),
                ch_name: String::from("TOKYO MX"),
                flag: None,
                duration_min: Some(30),
                sub_title: Some(String::from("転がるぼっち")),
            }],
        );

        let stats = ViewerStats {
            total_titles: 2,
            total_programs: 3,
            unique_channels: 2,
            oldest_st_time: Some(String::from("2022-04-09 23:00:00")),
            newest_st_time: Some(String::from("2022-10-08 23:30:00")),
        };

        TitleViewerState::new(titles, programs_by_tid, stats)
    }

    #[test]
    fn test_initial_state() {
        // Arrange & Act
        let state = make_state();

        // Assert
        assert_eq!(state.filtered_titles().len(), 2);
        assert_eq!(state.active_pane, ActivePane::Titles);
        assert_eq!(state.title_cursor(), 0);
    }

    #[test]
    fn test_move_down_and_up() {
        // Arrange
        let mut state = make_state();

        // Act & Assert
        state.move_down();
        assert_eq!(state.title_cursor(), 1);

        state.move_down(); // at end, should not move
        assert_eq!(state.title_cursor(), 1);

        state.move_up();
        assert_eq!(state.title_cursor(), 0);

        state.move_up(); // at start, should not move
        assert_eq!(state.title_cursor(), 0);
    }

    #[test]
    fn test_focus_programs_and_titles() {
        // Arrange
        let mut state = make_state();

        // Act: focus programs pane
        state.focus_programs();

        // Assert
        assert_eq!(state.active_pane, ActivePane::Programs);
        assert_eq!(state.program_cursor(), 0);
        assert_eq!(state.current_programs().len(), 2);

        // Act: navigate in programs pane
        state.move_down();
        assert_eq!(state.program_cursor(), 1);

        // Act: focus titles pane
        state.focus_titles();
        assert_eq!(state.active_pane, ActivePane::Titles);
    }

    #[test]
    fn test_page_up_and_page_down() {
        // Arrange
        let mut state = make_state();

        // Act: page down in titles (page_size=10, but only 2 items)
        state.page_down(10);

        // Assert: clamped to last item
        assert_eq!(state.title_cursor(), 1);

        // Act: page up
        state.page_up(10);

        // Assert: back to first
        assert_eq!(state.title_cursor(), 0);

        // Act: switch to programs and page
        state.focus_programs();
        state.page_down(10);
        assert_eq!(state.program_cursor(), 1); // only 2 programs for tid=1

        state.page_up(10);
        assert_eq!(state.program_cursor(), 0);
    }

    #[test]
    fn test_filter() {
        // Arrange
        let mut state = make_state();

        // Act
        state.set_filter(String::from("spy"));

        // Assert
        assert_eq!(state.filtered_titles().len(), 1);
        assert_eq!(state.current_title().unwrap().tid, 1);
    }

    #[test]
    fn test_filter_by_sub_title() {
        // Arrange
        let mut state = make_state();

        // Act: filter by sub_title that belongs to TID=1
        state.set_filter(String::from("梟"));

        // Assert: only SPY×FAMILY matches
        assert_eq!(state.filtered_titles().len(), 1);
        assert_eq!(state.current_title().unwrap().tid, 1);
    }

    #[test]
    fn test_filter_by_st_time() {
        // Arrange
        let mut state = make_state();

        // Act: filter by st_time prefix "2022-10" (matches TID=2 only)
        state.set_filter(String::from("2022-10"));

        // Assert: only Bocchi the Rock! matches
        assert_eq!(state.filtered_titles().len(), 1);
        assert_eq!(state.current_title().unwrap().tid, 2);
    }

    #[test]
    fn test_filter_no_match() {
        // Arrange
        let mut state = make_state();

        // Act
        state.set_filter(String::from("nonexistent"));

        // Assert
        assert!(state.filtered_titles().is_empty());
        assert!(state.current_title().is_none());
    }
}
