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
    let rows: Vec<NormalizeRow> = titles
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
        .collect();

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
