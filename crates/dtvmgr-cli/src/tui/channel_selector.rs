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
