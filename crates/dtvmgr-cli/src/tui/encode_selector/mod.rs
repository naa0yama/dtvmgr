//! Encode selector TUI main loop.

/// Encode selector state types.
pub mod state;
mod ui;

use std::io;
use std::sync::mpsc;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use self::state::{
    EncodeSelectorState, FileCheckWorkerProgress, InputMode, SelectorResult, SettingsField,
    SyncMessage, WizardStep,
};
use self::state::{FileCheckMessage, QueueMessage};

/// TUI terminal handle (alternate screen + raw mode).
pub type TuiTerminal = Terminal<CrosstermBackend<io::Stdout>>;

/// Enters alternate screen and raw mode.
///
/// # Errors
///
/// Returns an error if terminal setup fails.
pub fn setup_terminal() -> Result<TuiTerminal> {
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)
        .context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).context("failed to create terminal")
}

/// Leaves alternate screen and restores normal mode.
///
/// # Errors
///
/// Returns an error if terminal teardown fails.
pub fn teardown_terminal() -> Result<()> {
    disable_raw_mode().context("failed to disable raw mode")?;
    crossterm::execute!(io::stdout(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")
}

/// Runs the encode selector event loop on an existing terminal.
///
/// Does **not** set up or tear down the terminal — the caller manages its lifecycle.
/// An optional `sync_rx` receiver can be passed to receive background sync progress.
///
/// The event loop is async so that `tokio::spawn`-ed background tasks (file checks,
/// sync, queue polling) can make progress on a `current_thread` runtime.
///
/// # Errors
///
/// Returns an error if event handling fails.
#[allow(clippy::module_name_repetitions, clippy::future_not_send)]
pub async fn run_encode_selector(
    terminal: &mut TuiTerminal,
    state: &mut EncodeSelectorState,
    sync_rx: Option<&mpsc::Receiver<SyncMessage>>,
    file_check_rx: Option<&mpsc::Receiver<FileCheckMessage>>,
    queue_rx: Option<&mpsc::Receiver<QueueMessage>>,
    progress_rx: &tokio::sync::watch::Receiver<FileCheckWorkerProgress>,
) -> Result<SelectorResult> {
    run_event_loop(
        terminal,
        state,
        sync_rx,
        file_check_rx,
        queue_rx,
        progress_rx,
    )
    .await
}

/// Draws a loading indicator with optional file check progress.
///
/// # Errors
///
/// Returns an error if drawing fails.
pub fn draw_loading_progress(
    terminal: &mut TuiTerminal,
    page: u64,
    checked: usize,
    total: usize,
) -> Result<()> {
    terminal
        .draw(|frame| ui::draw_loading_progress(frame, page, checked, total))
        .context("failed to draw loading screen")?;
    Ok(())
}

/// Main event loop.
///
/// Uses non-blocking `event::poll` with async sleep so that `tokio::spawn`-ed
/// background tasks can make progress on a `current_thread` runtime.
#[allow(clippy::future_not_send)]
async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut EncodeSelectorState,
    sync_rx: Option<&mpsc::Receiver<SyncMessage>>,
    file_check_rx: Option<&mpsc::Receiver<FileCheckMessage>>,
    queue_rx: Option<&mpsc::Receiver<QueueMessage>>,
    progress_rx: &tokio::sync::watch::Receiver<FileCheckWorkerProgress>,
) -> Result<SelectorResult> {
    loop {
        // Drain background sync messages.
        if let Some(rx) = sync_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SyncMessage::Progress { fetched, total } => {
                        state.sync_progress = Some((fetched, total));
                    }
                    SyncMessage::Complete => {
                        state.sync_progress = None;
                    }
                }
            }
        }

        // Drain background file check messages (batch update, single rebuild).
        if let Some(rx) = file_check_rx {
            let mut had_updates = false;
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    FileCheckMessage::Result {
                        recorded_id,
                        exists,
                    } => {
                        state.update_file_exists(recorded_id, exists);
                        had_updates = true;
                    }
                    FileCheckMessage::Complete => {
                        had_updates = true;
                    }
                }
            }
            if had_updates {
                state.rebuild_filter();
            }
        }

        // Read global worker progress from watch channel.
        {
            let wp = *progress_rx.borrow();
            let new_progress = wp.is_active().then_some(wp);
            if state.file_check_progress != new_progress {
                state.file_check_progress = new_progress;
            }
        }

        // Drain encode queue messages.
        if let Some(rx) = queue_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    QueueMessage::Update(info) => {
                        state.encode_queue = Some(info);
                    }
                }
            }
        }

        terminal
            .draw(|frame| ui::draw(frame, state))
            .context("failed to draw TUI")?;

        // Non-blocking poll: check if a key event is already available.
        if event::poll(std::time::Duration::ZERO).context("failed to poll events")?
            && let Event::Key(key) = event::read().context("failed to read event")?
            && key.kind == KeyEventKind::Press
            && let Some(result) = handle_input(state, key.code, key.modifiers)
        {
            return Ok(result);
        }

        // Yield to the tokio runtime so spawned background tasks can progress.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// Routes input handling based on current step and input mode.
fn handle_input(
    state: &mut EncodeSelectorState,
    key: KeyCode,
    modifiers: KeyModifiers,
) -> Option<SelectorResult> {
    match state.step {
        WizardStep::SelectRecordings => handle_recording_normal(state, key, modifiers),
        WizardStep::ConfigureSettings => match state.input_mode {
            InputMode::DirectoryInput => handle_settings_dir_input(state, key),
            InputMode::Normal => handle_settings_normal(state, key, modifiers),
        },
        WizardStep::Confirm => handle_confirm(state, key, modifiers),
    }
}

/// Step 1: Normal navigation mode.
fn handle_recording_normal(
    state: &mut EncodeSelectorState,
    key: KeyCode,
    modifiers: KeyModifiers,
) -> Option<SelectorResult> {
    match key {
        KeyCode::Char('q') | KeyCode::Esc => return Some(SelectorResult::Cancelled),
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            return Some(SelectorResult::Cancelled);
        }
        KeyCode::Up | KeyCode::Char('k') => state.move_up(),
        KeyCode::Down | KeyCode::Char('j') => state.move_down(),
        KeyCode::Char(' ') => state.toggle_current(),
        KeyCode::Char('a') => state.select_all(),
        KeyCode::Char('A') => state.deselect_all(),
        KeyCode::Char('l') if state.has_next_page() => return Some(SelectorResult::PageNext),
        KeyCode::Char('h') if state.has_prev_page() => return Some(SelectorResult::PagePrev),
        KeyCode::Char('f') => state.toggle_hide_unavailable(),
        KeyCode::Char('R') => return Some(SelectorResult::Refresh),
        KeyCode::Enter => {
            if !state.selected.is_empty() {
                state.step = WizardStep::ConfigureSettings;
            }
        }
        _ => {}
    }
    None
}

/// Step 2: Settings normal mode.
fn handle_settings_normal(
    state: &mut EncodeSelectorState,
    key: KeyCode,
    modifiers: KeyModifiers,
) -> Option<SelectorResult> {
    match key {
        KeyCode::Char('q') => return Some(SelectorResult::Cancelled),
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            return Some(SelectorResult::Cancelled);
        }
        KeyCode::Esc => {
            state.step = WizardStep::SelectRecordings;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.settings_field = state.settings_field.prev();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.settings_field = state.settings_field.next();
        }
        KeyCode::Char(' ') | KeyCode::Right | KeyCode::Left => {
            handle_settings_toggle(state, key);
        }
        KeyCode::Enter => {
            if state.settings_field == SettingsField::Directory {
                state.input_mode = InputMode::DirectoryInput;
            } else {
                state.step = WizardStep::Confirm;
                state.confirm_count = 0;
            }
        }
        _ => {}
    }
    None
}

/// Toggles/cycles the current settings field.
fn handle_settings_toggle(state: &mut EncodeSelectorState, key: KeyCode) {
    match state.settings_field {
        SettingsField::Preset => {
            if key == KeyCode::Left {
                state.prev_preset();
            } else {
                state.next_preset();
            }
        }
        SettingsField::SaveSameDirectory => {
            state.settings.is_save_same_directory = !state.settings.is_save_same_directory;
        }
        SettingsField::ParentDir => {
            if !state.settings.is_save_same_directory {
                if key == KeyCode::Left {
                    state.prev_parent_dir();
                } else {
                    state.next_parent_dir();
                }
            }
        }
        SettingsField::Directory => {
            state.input_mode = InputMode::DirectoryInput;
        }
        SettingsField::RemoveOriginal => {
            state.settings.remove_original = !state.settings.remove_original;
        }
    }
}

/// Step 2: Directory text input mode.
fn handle_settings_dir_input(
    state: &mut EncodeSelectorState,
    key: KeyCode,
) -> Option<SelectorResult> {
    match key {
        KeyCode::Esc | KeyCode::Enter => {
            state.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            state.settings.directory.pop();
        }
        KeyCode::Char(c) => {
            state.settings.directory.push(c);
        }
        _ => {}
    }
    None
}

/// Step 3: Confirm screen.
const fn handle_confirm(
    state: &mut EncodeSelectorState,
    key: KeyCode,
    modifiers: KeyModifiers,
) -> Option<SelectorResult> {
    match key {
        KeyCode::Char('q') => return Some(SelectorResult::Cancelled),
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            return Some(SelectorResult::Cancelled);
        }
        KeyCode::Esc => {
            state.step = WizardStep::ConfigureSettings;
            state.confirm_count = 0;
        }
        KeyCode::Enter => {
            state.confirm_count = state.confirm_count.saturating_add(1);
            if state.confirm_count >= state.required_confirms() {
                return Some(SelectorResult::Confirmed);
            }
        }
        _ => {}
    }
    None
}
