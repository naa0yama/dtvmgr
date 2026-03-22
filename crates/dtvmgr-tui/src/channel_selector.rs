//! Channel selector TUI main loop.

use std::collections::BTreeSet;
use std::io;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use super::state::{ChannelGroup, ChannelSelectorState, InputMode, SelectorResult};
use super::ui;

/// Runs the channel selector TUI and returns the selected channel IDs.
///
/// Returns `None` if the user cancels, or `Some(selected)` if confirmed.
///
/// # Errors
///
/// Returns an error if terminal setup or event handling fails.
pub fn run_channel_selector(
    groups: Vec<ChannelGroup>,
    initial_selected: BTreeSet<u32>,
) -> Result<Option<Vec<u32>>> {
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)
        .context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    let mut state = ChannelSelectorState::new(groups, initial_selected);

    let result = run_event_loop(&mut terminal, &mut state);

    // Cleanup (always attempt even if event loop failed)
    disable_raw_mode().context("failed to disable raw mode")?;
    crossterm::execute!(io::stdout(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;

    let selector_result = result?;

    match selector_result {
        SelectorResult::Confirmed => {
            let selected: Vec<u32> = state.selected.into_iter().collect();
            Ok(Some(selected))
        }
        SelectorResult::Cancelled => Ok(None),
    }
}

/// Main event loop.
fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut ChannelSelectorState,
) -> Result<SelectorResult> {
    loop {
        terminal
            .draw(|frame| ui::draw(frame, state))
            .context("failed to draw TUI")?;

        if event::poll(std::time::Duration::from_millis(100)).context("failed to poll events")?
            && let Event::Key(key) = event::read().context("failed to read event")?
            && key.kind == KeyEventKind::Press
        {
            match state.input_mode {
                InputMode::Filter => {
                    if let Some(result) = handle_filter_input(state, key.code) {
                        return Ok(result);
                    }
                }
                InputMode::Normal => {
                    if let Some(result) = handle_normal_input(state, key.code, key.modifiers) {
                        return Ok(result);
                    }
                }
            }
        }
    }
}

/// Handles key input in filter mode. Returns `Some` to exit the loop.
fn handle_filter_input(state: &mut ChannelSelectorState, key: KeyCode) -> Option<SelectorResult> {
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
    None
}

/// Handles key input in normal mode. Returns `Some` to exit the loop.
fn handle_normal_input(
    state: &mut ChannelSelectorState,
    key: KeyCode,
    modifiers: KeyModifiers,
) -> Option<SelectorResult> {
    match key {
        KeyCode::Char('q') | KeyCode::Esc => return Some(SelectorResult::Cancelled),
        KeyCode::Enter => return Some(SelectorResult::Confirmed),
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            return Some(SelectorResult::Cancelled);
        }
        KeyCode::Tab | KeyCode::BackTab => state.switch_pane(),
        KeyCode::Up | KeyCode::Char('k') => state.move_up(),
        KeyCode::Down | KeyCode::Char('j') => state.move_down(),
        KeyCode::Char(' ') => state.toggle_current(),
        KeyCode::Char('a') => state.select_all_in_group(),
        KeyCode::Char('A') => {
            state.deselect_all_in_group();
        }
        KeyCode::Char('/') => {
            state.input_mode = InputMode::Filter;
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeSet;

    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;
    use crate::state::{ActivePane, ChannelEntry, ChannelGroup, InputMode, SelectorResult};

    fn make_test_state() -> ChannelSelectorState {
        let groups = vec![
            ChannelGroup {
                ch_gid: 1,
                name: String::from("Group A"),
                channels: vec![
                    ChannelEntry {
                        ch_id: 10,
                        ch_name: String::from("Ch10"),
                    },
                    ChannelEntry {
                        ch_id: 11,
                        ch_name: String::from("Ch11"),
                    },
                ],
            },
            ChannelGroup {
                ch_gid: 2,
                name: String::from("Group B"),
                channels: vec![ChannelEntry {
                    ch_id: 20,
                    ch_name: String::from("Ch20"),
                }],
            },
        ];
        ChannelSelectorState::new(groups, BTreeSet::from([10]))
    }

    // ── handle_filter_input ─────────────────────────────────────

    #[test]
    fn filter_input_esc_clears_filter_and_returns_normal() {
        // Arrange
        let mut state = make_test_state();
        state.input_mode = InputMode::Filter;
        state.filter_push('a');

        // Act
        let result = handle_filter_input(&mut state, KeyCode::Esc);

        // Assert
        assert!(result.is_none());
        assert!(state.filter.is_empty());
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn filter_input_enter_returns_to_normal_keeping_filter() {
        // Arrange
        let mut state = make_test_state();
        state.input_mode = InputMode::Filter;
        state.filter_push('x');

        // Act
        let result = handle_filter_input(&mut state, KeyCode::Enter);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.input_mode, InputMode::Normal);
        assert_eq!(state.filter, "x");
    }

    #[test]
    fn filter_input_backspace_pops_char() {
        // Arrange
        let mut state = make_test_state();
        state.filter_push('a');
        state.filter_push('b');

        // Act
        let result = handle_filter_input(&mut state, KeyCode::Backspace);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.filter, "a");
    }

    #[test]
    fn filter_input_char_pushes() {
        // Arrange
        let mut state = make_test_state();

        // Act
        let result = handle_filter_input(&mut state, KeyCode::Char('z'));

        // Assert
        assert!(result.is_none());
        assert_eq!(state.filter, "z");
    }

    #[test]
    fn filter_input_unknown_key_does_nothing() {
        // Arrange
        let mut state = make_test_state();

        // Act
        let result = handle_filter_input(&mut state, KeyCode::F(1));

        // Assert
        assert!(result.is_none());
        assert!(state.filter.is_empty());
    }

    // ── handle_normal_input ─────────────────────────────────────

    #[test]
    fn normal_input_q_returns_cancelled() {
        // Arrange
        let mut state = make_test_state();

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Char('q'), KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Cancelled));
    }

    #[test]
    fn normal_input_esc_returns_cancelled() {
        // Arrange
        let mut state = make_test_state();

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Esc, KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Cancelled));
    }

    #[test]
    fn normal_input_enter_returns_confirmed() {
        // Arrange
        let mut state = make_test_state();

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Enter, KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Confirmed));
    }

    #[test]
    fn normal_input_ctrl_c_returns_cancelled() {
        // Arrange
        let mut state = make_test_state();

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL);

        // Assert
        assert_eq!(result, Some(SelectorResult::Cancelled));
    }

    #[test]
    fn normal_input_tab_switches_pane() {
        // Arrange
        let mut state = make_test_state();
        assert_eq!(state.active_pane, ActivePane::Groups);

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Tab, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.active_pane, ActivePane::Channels);
    }

    #[test]
    fn normal_input_backtab_switches_pane() {
        // Arrange
        let mut state = make_test_state();
        state.active_pane = ActivePane::Channels;

        // Act
        let result = handle_normal_input(&mut state, KeyCode::BackTab, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.active_pane, ActivePane::Groups);
    }

    #[test]
    fn normal_input_up_moves_up() {
        // Arrange
        let mut state = make_test_state();
        state.move_down();
        assert_eq!(state.group_cursor, 1);

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Up, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.group_cursor, 0);
    }

    #[test]
    fn normal_input_k_moves_up() {
        // Arrange
        let mut state = make_test_state();
        state.move_down();

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Char('k'), KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.group_cursor, 0);
    }

    #[test]
    fn normal_input_down_moves_down() {
        // Arrange
        let mut state = make_test_state();

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Down, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.group_cursor, 1);
    }

    #[test]
    fn normal_input_j_moves_down() {
        // Arrange
        let mut state = make_test_state();

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Char('j'), KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.group_cursor, 1);
    }

    #[test]
    fn normal_input_space_toggles_current() {
        // Arrange
        let mut state = make_test_state();

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Char(' '), KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert!(state.selected.contains(&10));
        assert!(state.selected.contains(&11));
    }

    #[test]
    fn normal_input_a_selects_all_in_group() {
        // Arrange
        let mut state = make_test_state();
        state.move_down(); // cursor on Group B

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Char('a'), KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert!(state.selected.contains(&20));
    }

    #[test]
    fn normal_input_shift_a_deselects_all_in_group() {
        // Arrange
        let mut state = make_test_state();

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Char('A'), KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert!(!state.selected.contains(&10));
    }

    #[test]
    fn normal_input_slash_enters_filter_mode() {
        // Arrange
        let mut state = make_test_state();

        // Act
        let result = handle_normal_input(&mut state, KeyCode::Char('/'), KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.input_mode, InputMode::Filter);
    }

    #[test]
    fn normal_input_unknown_key_returns_none() {
        // Arrange
        let mut state = make_test_state();

        // Act
        let result = handle_normal_input(&mut state, KeyCode::F(5), KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
    }
}
