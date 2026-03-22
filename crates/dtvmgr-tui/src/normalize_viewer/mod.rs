//! Normalize viewer TUI main loop.

/// Normalize viewer state types.
pub mod state;
mod ui;

use std::io;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use self::state::{
    InputMode, NormalizeRow, NormalizeViewerState, RegexSource, categorize, normalize_chars,
};
use dtvmgr_db::titles::CachedTitle;

/// Builds normalize rows from cached titles.
fn build_normalize_rows(titles: &[CachedTitle]) -> Vec<NormalizeRow> {
    titles
        .iter()
        .map(|t| {
            let normalized_title = normalize_chars(&t.title);
            let media_type = categorize(t.cat);
            NormalizeRow {
                tid: t.tid,
                title: t.title.clone(),
                normalized_title,
                cat: t.cat,
                first_year: t.first_year,
                media_type,
                base_query: None,
                season_num: None,
                trimmed: None,
            }
        })
        .collect()
}

/// Runs the normalize viewer TUI.
///
/// Builds `NormalizeRow` entries from cached titles, launches the TUI,
/// and returns a tuple of (TSV output lines, updated regex history).
///
/// # Errors
///
/// Returns an error if terminal setup or event handling fails.
#[allow(clippy::module_name_repetitions)]
pub fn run_normalize_viewer(
    titles: &[CachedTitle],
    regex_history: &[String],
    regex_titles: &[String],
) -> Result<(Vec<String>, Vec<String>)> {
    let rows = build_normalize_rows(titles);

    let mut state = NormalizeViewerState::new(rows, regex_history.to_vec(), regex_titles);

    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)
        .context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    let result = run_event_loop(&mut terminal, &mut state);

    // Cleanup (always attempt even if event loop failed)
    disable_raw_mode().context("failed to disable raw mode")?;
    crossterm::execute!(io::stdout(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;

    result?;
    Ok((state.build_output(), state.regex_history().to_vec()))
}

/// Main event loop.
fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut NormalizeViewerState,
) -> Result<()> {
    let mut main_area_height: u16 = 0;

    loop {
        terminal
            .draw(|frame| {
                main_area_height = ui::draw(frame, state);
            })
            .context("failed to draw TUI")?;

        let page_size = usize::from(main_area_height.saturating_sub(4));

        if event::poll(std::time::Duration::from_millis(100)).context("failed to poll events")?
            && let Event::Key(key) = event::read().context("failed to read event")?
            && key.kind == KeyEventKind::Press
        {
            match state.input_mode {
                InputMode::Filter => {
                    if handle_filter_input(state, key.code) {
                        return Ok(());
                    }
                }
                InputMode::Regex => {
                    if handle_regex_input(state, key.code) {
                        return Ok(());
                    }
                }
                InputMode::Normal => {
                    if handle_normal_input(state, key.code, key.modifiers, page_size) {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Handles key input in filter mode. Returns `true` to exit.
fn handle_filter_input(state: &mut NormalizeViewerState, key: KeyCode) -> bool {
    match key {
        KeyCode::Esc => {
            state.set_filter(String::new());
            state.input_mode = InputMode::Normal;
        }
        KeyCode::Enter => {
            state.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            state.filter_pop();
        }
        KeyCode::Char(c) => {
            state.filter_push(c);
        }
        _ => {}
    }
    false
}

/// Handles key input in regex mode. Returns `true` to exit.
fn handle_regex_input(state: &mut NormalizeViewerState, key: KeyCode) -> bool {
    match key {
        KeyCode::Esc => {
            state.input_mode = InputMode::Normal;
        }
        KeyCode::Enter => {
            state.apply_regex();
            state.commit_regex_to_history();
            state.input_mode = InputMode::Normal;
        }
        KeyCode::Up => {
            state.regex_history_up();
        }
        KeyCode::Down => {
            state.regex_history_down();
        }
        KeyCode::Left => {
            state.regex_cursor_left();
        }
        KeyCode::Right => {
            state.regex_cursor_right();
        }
        KeyCode::Home => {
            state.regex_cursor_home();
        }
        KeyCode::End => {
            state.regex_cursor_end();
        }
        KeyCode::Backspace => {
            state.regex_delete_back();
        }
        KeyCode::Char(c) => {
            state.regex_insert_char(c);
        }
        _ => {}
    }
    false
}

/// Handles key input in normal mode. Returns `true` to exit.
fn handle_normal_input(
    state: &mut NormalizeViewerState,
    key: KeyCode,
    modifiers: KeyModifiers,
    page_size: usize,
) -> bool {
    let shift = modifiers.contains(KeyModifiers::SHIFT);

    match key {
        KeyCode::Char('q') => return true,
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Up | KeyCode::Char('k') if shift => state.shift_move_up(),
        KeyCode::Down | KeyCode::Char('j') if shift => state.shift_move_down(),
        KeyCode::Char('K') => state.shift_move_up(),
        KeyCode::Char('J') => state.shift_move_down(),
        KeyCode::Up | KeyCode::Char('k') => state.move_up(),
        KeyCode::Down | KeyCode::Char('j') => state.move_down(),
        KeyCode::PageUp => state.page_up(page_size),
        KeyCode::PageDown => state.page_down(page_size),
        KeyCode::Char(' ') => state.toggle_select(),
        KeyCode::Char('/') => {
            state.input_mode = InputMode::Filter;
        }
        KeyCode::Char('r') => {
            if state.regex_source != RegexSource::Manual {
                state.regex_source = RegexSource::Manual;
                state.apply_regex();
            }
            state.input_mode = InputMode::Regex;
        }
        KeyCode::Char('R') => {
            state.toggle_regex_source();
        }
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;
    use crate::normalize_viewer::state::{
        InputMode, MediaType, NormalizeRow, NormalizeViewerState, RegexSource, normalize_chars,
    };

    fn make_row(tid: u32, title: &str) -> NormalizeRow {
        let normalized_title = normalize_chars(title);
        NormalizeRow {
            tid,
            title: title.to_owned(),
            normalized_title,
            cat: Some(1),
            first_year: Some(2024),
            media_type: MediaType::Tv,
            base_query: None,
            season_num: None,
            trimmed: None,
        }
    }

    fn make_state() -> NormalizeViewerState {
        let rows = vec![
            make_row(1, "Title A"),
            make_row(2, "Title B"),
            make_row(3, "Title C"),
        ];
        NormalizeViewerState::new(rows, Vec::new(), &[])
    }

    // ── handle_filter_input ─────────────────────────────────────

    #[test]
    fn filter_input_esc_clears_filter_and_returns_normal() {
        // Arrange
        let mut state = make_state();
        state.input_mode = InputMode::Filter;
        state.filter_push('a');

        // Act
        let exit = handle_filter_input(&mut state, KeyCode::Esc);

        // Assert
        assert!(!exit);
        assert!(state.filter.is_empty());
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn filter_input_enter_returns_to_normal() {
        // Arrange
        let mut state = make_state();
        state.input_mode = InputMode::Filter;
        state.filter_push('x');

        // Act
        let exit = handle_filter_input(&mut state, KeyCode::Enter);

        // Assert
        assert!(!exit);
        assert_eq!(state.input_mode, InputMode::Normal);
        assert_eq!(state.filter, "x");
    }

    #[test]
    fn filter_input_backspace_pops() {
        // Arrange
        let mut state = make_state();
        state.filter_push('a');
        state.filter_push('b');

        // Act
        let exit = handle_filter_input(&mut state, KeyCode::Backspace);

        // Assert
        assert!(!exit);
        assert_eq!(state.filter, "a");
    }

    #[test]
    fn filter_input_char_pushes() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_filter_input(&mut state, KeyCode::Char('z'));

        // Assert
        assert!(!exit);
        assert_eq!(state.filter, "z");
    }

    #[test]
    fn filter_input_unknown_key_does_nothing() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_filter_input(&mut state, KeyCode::F(1));

        // Assert
        assert!(!exit);
        assert!(state.filter.is_empty());
    }

    // ── handle_regex_input ──────────────────────────────────────

    #[test]
    fn regex_input_esc_returns_to_normal() {
        // Arrange
        let mut state = make_state();
        state.input_mode = InputMode::Regex;

        // Act
        let exit = handle_regex_input(&mut state, KeyCode::Esc);

        // Assert
        assert!(!exit);
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn regex_input_enter_applies_and_commits() {
        // Arrange
        let mut state = make_state();
        state.input_mode = InputMode::Regex;
        state.regex_input = String::from("test_pattern");

        // Act
        let exit = handle_regex_input(&mut state, KeyCode::Enter);

        // Assert
        assert!(!exit);
        assert_eq!(state.input_mode, InputMode::Normal);
        assert!(
            state
                .regex_history()
                .contains(&String::from("test_pattern"))
        );
    }

    #[test]
    fn regex_input_up_navigates_history() {
        // Arrange
        let rows = vec![make_row(1, "X")];
        let history = vec![String::from("old_pattern")];
        let mut state = NormalizeViewerState::new(rows, history, &[]);
        state.input_mode = InputMode::Regex;

        // Act
        let exit = handle_regex_input(&mut state, KeyCode::Up);

        // Assert
        assert!(!exit);
        assert_eq!(state.regex_input, "old_pattern");
    }

    #[test]
    fn regex_input_down_navigates_history() {
        // Arrange
        let rows = vec![make_row(1, "X")];
        let history = vec![String::from("a"), String::from("b")];
        let mut state = NormalizeViewerState::new(rows, history, &[]);
        state.input_mode = InputMode::Regex;
        state.regex_history_up(); // go to "b"
        state.regex_history_up(); // go to "a"

        // Act
        let exit = handle_regex_input(&mut state, KeyCode::Down);

        // Assert
        assert!(!exit);
        assert_eq!(state.regex_input, "b");
    }

    #[test]
    fn regex_input_left_moves_cursor() {
        // Arrange
        let mut state = make_state();
        state.regex_input = String::from("abc");
        state.regex_cursor_end();
        let initial = state.regex_cursor_display_width();

        // Act
        let exit = handle_regex_input(&mut state, KeyCode::Left);

        // Assert
        assert!(!exit);
        assert!(state.regex_cursor_display_width() < initial);
    }

    #[test]
    fn regex_input_right_moves_cursor() {
        // Arrange
        let mut state = make_state();
        state.regex_input = String::from("abc");
        state.regex_cursor_home();

        // Act
        let exit = handle_regex_input(&mut state, KeyCode::Right);

        // Assert
        assert!(!exit);
        assert!(state.regex_cursor_display_width() > 0);
    }

    #[test]
    fn regex_input_home_end() {
        // Arrange
        let mut state = make_state();
        state.regex_input = String::from("abc");
        state.regex_cursor_end();

        // Act
        let exit = handle_regex_input(&mut state, KeyCode::Home);

        // Assert
        assert!(!exit);
        assert_eq!(state.regex_cursor_display_width(), 0);

        // Act
        let exit = handle_regex_input(&mut state, KeyCode::End);

        // Assert
        assert!(!exit);
        assert_eq!(state.regex_cursor_display_width(), 3);
    }

    #[test]
    fn regex_input_backspace_deletes() {
        // Arrange
        let mut state = make_state();
        state.regex_input = String::from("abc");
        state.regex_cursor_end();

        // Act
        let exit = handle_regex_input(&mut state, KeyCode::Backspace);

        // Assert
        assert!(!exit);
        assert_eq!(state.regex_input, "ab");
    }

    #[test]
    fn regex_input_char_inserts() {
        // Arrange
        let mut state = make_state();
        state.regex_input = String::from("ab");
        state.regex_cursor_end();

        // Act
        let exit = handle_regex_input(&mut state, KeyCode::Char('c'));

        // Assert
        assert!(!exit);
        assert_eq!(state.regex_input, "abc");
    }

    #[test]
    fn regex_input_unknown_key_does_nothing() {
        // Arrange
        let mut state = make_state();
        let before = state.regex_input.clone();

        // Act
        let exit = handle_regex_input(&mut state, KeyCode::F(1));

        // Assert
        assert!(!exit);
        assert_eq!(state.regex_input, before);
    }

    // ── handle_normal_input ─────────────────────────────────────

    #[test]
    fn normal_input_q_exits() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('q'), KeyModifiers::NONE, 10);

        // Assert
        assert!(exit);
    }

    #[test]
    fn normal_input_ctrl_c_exits() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL, 10);

        // Assert
        assert!(exit);
    }

    #[test]
    fn normal_input_up_moves_up() {
        // Arrange
        let mut state = make_state();
        state.move_down();
        assert_eq!(state.cursor(), 1);

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Up, KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn normal_input_down_moves_down() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Down, KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.cursor(), 1);
    }

    #[test]
    fn normal_input_k_moves_up() {
        // Arrange
        let mut state = make_state();
        state.move_down();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('k'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn normal_input_j_moves_down() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('j'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.cursor(), 1);
    }

    #[test]
    fn normal_input_shift_k_shift_moves_up() {
        // Arrange
        let mut state = make_state();
        state.move_down();
        state.toggle_select(); // anchor at 1

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('K'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.cursor(), 0);
        assert!(state.selected.contains(&0));
    }

    #[test]
    fn normal_input_shift_j_shift_moves_down() {
        // Arrange
        let mut state = make_state();
        state.toggle_select(); // anchor at 0

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('J'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.cursor(), 1);
        assert!(state.selected.contains(&1));
    }

    #[test]
    fn normal_input_page_up_down() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::PageDown, KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.cursor(), 2);

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::PageUp, KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn normal_input_space_toggles_select() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char(' '), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert!(state.selected.contains(&0));
    }

    #[test]
    fn normal_input_slash_enters_filter_mode() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('/'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.input_mode, InputMode::Filter);
    }

    #[test]
    fn normal_input_r_enters_regex_mode() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('r'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.input_mode, InputMode::Regex);
        assert_eq!(state.regex_source, RegexSource::Manual);
    }

    #[test]
    fn normal_input_shift_r_toggles_regex_source() {
        // Arrange
        let mut state = make_state();
        assert_eq!(state.regex_source, RegexSource::Manual);

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('R'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.regex_source, RegexSource::Config);
    }

    #[test]
    fn normal_input_unknown_key_does_nothing() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::F(5), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
    }

    // ── build_normalize_rows ──────────────────────────────────────

    #[test]
    fn build_normalize_rows_creates_rows() {
        // Arrange
        let titles = vec![
            CachedTitle {
                tid: 1,
                tmdb_series_id: None,
                tmdb_season_number: None,
                tmdb_season_id: None,
                title: String::from("SPY×FAMILY Season 2"),
                short_title: None,
                title_yomi: None,
                title_en: None,
                cat: Some(1),
                title_flag: None,
                first_year: Some(2023),
                first_month: None,
                keywords: Vec::new(),
                sub_titles: None,
                last_update: String::new(),
                tmdb_original_name: None,
                tmdb_name: None,
                tmdb_alt_titles: None,
                tmdb_last_updated: None,
            },
            CachedTitle {
                tid: 2,
                tmdb_series_id: None,
                tmdb_season_number: None,
                tmdb_season_id: None,
                title: String::from("劇場版 鬼滅の刃"),
                short_title: None,
                title_yomi: None,
                title_en: None,
                cat: Some(8),
                title_flag: None,
                first_year: Some(2020),
                first_month: None,
                keywords: Vec::new(),
                sub_titles: None,
                last_update: String::new(),
                tmdb_original_name: None,
                tmdb_name: None,
                tmdb_alt_titles: None,
                tmdb_last_updated: None,
            },
        ];

        // Act
        let rows = build_normalize_rows(&titles);

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].tid, 1);
        assert_eq!(rows[0].title, "SPY×FAMILY Season 2");
        assert_eq!(rows[0].normalized_title, "SPY×FAMILY Season 2");
        assert_eq!(rows[0].cat, Some(1));
        assert_eq!(rows[0].first_year, Some(2023));
        assert_eq!(rows[0].media_type, MediaType::Tv);
        assert!(rows[0].base_query.is_none());
        assert!(rows[0].season_num.is_none());
        assert!(rows[0].trimmed.is_none());

        assert_eq!(rows[1].tid, 2);
        assert_eq!(rows[1].media_type, MediaType::Movie);
    }

    #[test]
    fn build_normalize_rows_empty() {
        // Arrange & Act
        let rows = build_normalize_rows(&[]);

        // Assert
        assert!(rows.is_empty());
    }

    #[test]
    fn build_normalize_rows_normalizes_fullwidth() {
        // Arrange
        let titles = vec![CachedTitle {
            tid: 10,
            tmdb_series_id: None,
            tmdb_season_number: None,
            tmdb_season_id: None,
            title: String::from("Ｈｅｌｌｏ"),
            short_title: None,
            title_yomi: None,
            title_en: None,
            cat: Some(7),
            title_flag: None,
            first_year: None,
            first_month: None,
            keywords: Vec::new(),
            sub_titles: None,
            last_update: String::new(),
            tmdb_original_name: None,
            tmdb_name: None,
            tmdb_alt_titles: None,
            tmdb_last_updated: None,
        }];

        // Act
        let rows = build_normalize_rows(&titles);

        // Assert
        assert_eq!(rows[0].normalized_title, "Hello");
        assert_eq!(rows[0].media_type, MediaType::Ova);
    }
}
