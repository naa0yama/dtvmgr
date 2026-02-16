//! Title/program viewer TUI main loop.

/// Title viewer state types.
pub mod state;
mod ui;

use std::collections::{HashMap, HashSet};
use std::io;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use self::state::{ActivePane, InputMode, ProgramRow, TitleRow, TitleViewerState, ViewerStats};
use dtvmgr_db::channels::CachedChannel;
use dtvmgr_db::programs::CachedProgram;
use dtvmgr_db::titles::CachedTitle;

/// Runs the title viewer TUI.
///
/// # Errors
///
/// Returns an error if terminal setup or event handling fails.
#[allow(clippy::module_name_repetitions)]
pub fn run_title_viewer(
    titles: &[CachedTitle],
    programs: &[CachedProgram],
    channels: Vec<CachedChannel>,
) -> Result<()> {
    // Build channel name lookup
    let ch_names: HashMap<u32, String> = channels
        .into_iter()
        .map(|ch| (ch.ch_id, ch.ch_name))
        .collect();

    // Group programs by TID
    let mut programs_by_tid: HashMap<u32, Vec<ProgramRow>> = HashMap::new();
    for p in programs {
        let ch_name = ch_names
            .get(&p.ch_id)
            .cloned()
            .unwrap_or_else(|| p.ch_id.to_string());

        programs_by_tid.entry(p.tid).or_default().push(ProgramRow {
            pid: p.pid,
            count: p.count,
            st_time: p.st_time.clone(),
            ch_name,
            flag: p.flag,
            duration_min: p.duration_min,
            sub_title: p.st_sub_title.clone().or_else(|| p.sub_title.clone()),
        });
    }

    // Compute stats from raw programs
    let unique_channels = programs
        .iter()
        .map(|p| p.ch_id)
        .collect::<HashSet<_>>()
        .len();
    let oldest_st_time = programs
        .iter()
        .map(|p| p.st_time.as_str())
        .min()
        .map(String::from);
    let newest_st_time = programs
        .iter()
        .map(|p| p.st_time.as_str())
        .max()
        .map(String::from);

    let viewer_stats = ViewerStats {
        total_titles: titles.len(),
        total_programs: programs.len(),
        unique_channels,
        oldest_st_time,
        newest_st_time,
    };

    // Build title rows
    let title_rows: Vec<TitleRow> = titles
        .iter()
        .map(|t| TitleRow {
            tid: t.tid,
            title: t.title.clone(),
            first_year: t.first_year,
            tmdb_series_id: t.tmdb_series_id,
            tmdb_season_number: t.tmdb_season_number,
            program_count: programs_by_tid.get(&t.tid).map_or(0, Vec::len),
        })
        .collect();

    let mut state = TitleViewerState::new(title_rows, programs_by_tid, viewer_stats);

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

    result
}

/// Main event loop.
fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut TitleViewerState,
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
fn handle_filter_input(state: &mut TitleViewerState, key: KeyCode) -> bool {
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

/// Handles key input in normal mode. Returns `true` to exit.
fn handle_normal_input(
    state: &mut TitleViewerState,
    key: KeyCode,
    modifiers: KeyModifiers,
    page_size: usize,
) -> bool {
    match key {
        KeyCode::Char('q') => return true,
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Up | KeyCode::Char('k') => state.move_up(),
        KeyCode::Down | KeyCode::Char('j') => state.move_down(),
        KeyCode::Right => state.focus_programs(),
        KeyCode::Left => state.focus_titles(),
        KeyCode::PageUp => state.page_up(page_size),
        KeyCode::PageDown => state.page_down(page_size),
        KeyCode::Char('/') => {
            if state.active_pane == ActivePane::Titles {
                state.input_mode = InputMode::Filter;
            }
        }
        KeyCode::Char('o') => open_syoboi_url(state),
        _ => {}
    }
    false
}

/// Opens the Syoboi Calendar page for the current title or program.
#[allow(clippy::indexing_slicing)]
fn open_syoboi_url(state: &TitleViewerState) {
    let Some(title) = state.current_title() else {
        return;
    };
    let url = match state.active_pane {
        ActivePane::Titles => {
            format!("{}/tid/{}", dtvmgr_api::syoboi::SYOBOI_BASE_URL, title.tid)
        }
        ActivePane::Programs => {
            let programs = state.current_programs();
            let Some(prog) = programs.get(state.program_cursor()) else {
                return;
            };
            // st_time format: "YYYY-MM-DD HH:MM:SS" -> extract YYYYMM
            let yyyymm = prog.st_time.replace('-', "");
            let yyyymm = &yyyymm[..6];
            format!(
                "{}/tid/{}/summary/{yyyymm}",
                dtvmgr_api::syoboi::SYOBOI_BASE_URL,
                title.tid
            )
        }
    };
    let _ = open::that(&url);
}
