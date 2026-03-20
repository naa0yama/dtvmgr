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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use crossterm::event::{KeyCode, KeyModifiers};

    use super::*;
    use crate::encode_selector::state::{
        EncodeSelectorState, InputMode, PageInfo, SelectorResult, SettingsField, WizardStep,
    };

    fn make_state() -> EncodeSelectorState {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 100,
        };
        EncodeSelectorState::new(
            vec![],
            vec![String::from("H.264"), String::from("H.265")],
            vec![String::from("recorded"), String::from("archive")],
            None,
            None,
            page,
        )
    }

    // ── handle_input routing ────────────────────────────────────

    #[test]
    fn handle_input_routes_to_recording_normal() {
        // Arrange
        let mut state = make_state();
        state.step = WizardStep::SelectRecordings;

        // Act
        let result = handle_input(&mut state, KeyCode::Char('q'), KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Cancelled));
    }

    #[test]
    fn handle_input_routes_to_settings_normal() {
        // Arrange
        let mut state = make_state();
        state.step = WizardStep::ConfigureSettings;
        state.input_mode = InputMode::Normal;

        // Act
        let result = handle_input(&mut state, KeyCode::Char('q'), KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Cancelled));
    }

    #[test]
    fn handle_input_routes_to_settings_dir_input() {
        // Arrange
        let mut state = make_state();
        state.step = WizardStep::ConfigureSettings;
        state.input_mode = InputMode::DirectoryInput;

        // Act
        let result = handle_input(&mut state, KeyCode::Char('x'), KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.settings.directory, "x");
    }

    #[test]
    fn handle_input_routes_to_confirm() {
        // Arrange
        let mut state = make_state();
        state.step = WizardStep::Confirm;

        // Act
        let result = handle_input(&mut state, KeyCode::Char('q'), KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Cancelled));
    }

    // ── handle_recording_normal ─────────────────────────────────

    #[test]
    fn recording_q_cancels() {
        // Arrange
        let mut state = make_state();

        // Act
        let result = handle_recording_normal(&mut state, KeyCode::Char('q'), KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Cancelled));
    }

    #[test]
    fn recording_esc_cancels() {
        // Arrange
        let mut state = make_state();

        // Act
        let result = handle_recording_normal(&mut state, KeyCode::Esc, KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Cancelled));
    }

    #[test]
    fn recording_ctrl_c_cancels() {
        // Arrange
        let mut state = make_state();

        // Act
        let result = handle_recording_normal(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL);

        // Assert
        assert_eq!(result, Some(SelectorResult::Cancelled));
    }

    #[test]
    fn recording_enter_with_no_selection_stays() {
        // Arrange
        let mut state = make_state();
        assert!(state.selected.is_empty());

        // Act
        let result = handle_recording_normal(&mut state, KeyCode::Enter, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.step, WizardStep::SelectRecordings);
    }

    #[test]
    fn recording_enter_with_selection_moves_to_settings() {
        // Arrange
        let mut state = make_state();
        state.selected.insert(1);

        // Act
        let result = handle_recording_normal(&mut state, KeyCode::Enter, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.step, WizardStep::ConfigureSettings);
    }

    #[test]
    fn recording_l_returns_page_next_when_available() {
        // Arrange
        let mut state = make_state();
        assert!(state.has_next_page());

        // Act
        let result = handle_recording_normal(&mut state, KeyCode::Char('l'), KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::PageNext));
    }

    #[test]
    fn recording_f_toggles_hide_unavailable() {
        // Arrange
        let mut state = make_state();
        assert!(!state.hide_unavailable);

        // Act
        let result = handle_recording_normal(&mut state, KeyCode::Char('f'), KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert!(state.hide_unavailable);
    }

    #[test]
    fn recording_shift_r_returns_refresh() {
        // Arrange
        let mut state = make_state();

        // Act
        let result = handle_recording_normal(&mut state, KeyCode::Char('R'), KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Refresh));
    }

    #[test]
    fn recording_unknown_key_returns_none() {
        // Arrange
        let mut state = make_state();

        // Act
        let result = handle_recording_normal(&mut state, KeyCode::F(1), KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
    }

    // ── handle_settings_normal ──────────────────────────────────

    #[test]
    fn settings_q_cancels() {
        // Arrange
        let mut state = make_state();

        // Act
        let result = handle_settings_normal(&mut state, KeyCode::Char('q'), KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Cancelled));
    }

    #[test]
    fn settings_esc_goes_back_to_recordings() {
        // Arrange
        let mut state = make_state();
        state.step = WizardStep::ConfigureSettings;

        // Act
        let result = handle_settings_normal(&mut state, KeyCode::Esc, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.step, WizardStep::SelectRecordings);
    }

    #[test]
    fn settings_up_moves_field_prev() {
        // Arrange
        let mut state = make_state();
        state.settings_field = SettingsField::SaveSameDirectory;

        // Act
        let result = handle_settings_normal(&mut state, KeyCode::Up, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.settings_field, SettingsField::Preset);
    }

    #[test]
    fn settings_down_moves_field_next() {
        // Arrange
        let mut state = make_state();
        state.settings_field = SettingsField::Preset;

        // Act
        let result = handle_settings_normal(&mut state, KeyCode::Down, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.settings_field, SettingsField::SaveSameDirectory);
    }

    #[test]
    fn settings_enter_on_directory_enters_input_mode() {
        // Arrange
        let mut state = make_state();
        state.settings_field = SettingsField::Directory;

        // Act
        let result = handle_settings_normal(&mut state, KeyCode::Enter, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.input_mode, InputMode::DirectoryInput);
    }

    #[test]
    fn settings_enter_on_non_directory_goes_to_confirm() {
        // Arrange
        let mut state = make_state();
        state.settings_field = SettingsField::Preset;

        // Act
        let result = handle_settings_normal(&mut state, KeyCode::Enter, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.step, WizardStep::Confirm);
        assert_eq!(state.confirm_count, 0);
    }

    // ── handle_settings_toggle ──────────────────────────────────

    #[test]
    fn settings_toggle_preset_right_cycles_next() {
        // Arrange
        let mut state = make_state();
        state.settings_field = SettingsField::Preset;
        assert_eq!(state.settings.mode, "H.264");

        // Act
        handle_settings_toggle(&mut state, KeyCode::Right);

        // Assert
        assert_eq!(state.settings.mode, "H.265");
    }

    #[test]
    fn settings_toggle_preset_left_cycles_prev() {
        // Arrange
        let mut state = make_state();
        state.settings_field = SettingsField::Preset;

        // Act
        handle_settings_toggle(&mut state, KeyCode::Left);

        // Assert
        assert_eq!(state.settings.mode, "H.265");
    }

    #[test]
    fn settings_toggle_save_same_directory() {
        // Arrange
        let mut state = make_state();
        state.settings_field = SettingsField::SaveSameDirectory;
        assert!(!state.settings.is_save_same_directory);

        // Act
        handle_settings_toggle(&mut state, KeyCode::Char(' '));

        // Assert
        assert!(state.settings.is_save_same_directory);
    }

    #[test]
    fn settings_toggle_remove_original() {
        // Arrange
        let mut state = make_state();
        state.settings_field = SettingsField::RemoveOriginal;
        assert!(!state.settings.remove_original);

        // Act
        handle_settings_toggle(&mut state, KeyCode::Char(' '));

        // Assert
        assert!(state.settings.remove_original);
    }

    #[test]
    fn settings_toggle_directory_enters_input_mode() {
        // Arrange
        let mut state = make_state();
        state.settings_field = SettingsField::Directory;

        // Act
        handle_settings_toggle(&mut state, KeyCode::Char(' '));

        // Assert
        assert_eq!(state.input_mode, InputMode::DirectoryInput);
    }

    #[test]
    fn settings_toggle_parent_dir_when_not_same_directory() {
        // Arrange
        let mut state = make_state();
        state.settings_field = SettingsField::ParentDir;
        state.settings.is_save_same_directory = false;
        assert_eq!(state.settings.parent_dir, "recorded");

        // Act
        handle_settings_toggle(&mut state, KeyCode::Right);

        // Assert
        assert_eq!(state.settings.parent_dir, "archive");
    }

    #[test]
    fn settings_toggle_parent_dir_noop_when_save_same() {
        // Arrange
        let mut state = make_state();
        state.settings_field = SettingsField::ParentDir;
        state.settings.is_save_same_directory = true;

        // Act
        handle_settings_toggle(&mut state, KeyCode::Right);

        // Assert: no change
        assert_eq!(state.settings.parent_dir, "recorded");
    }

    // ── handle_settings_dir_input ───────────────────────────────

    #[test]
    fn dir_input_esc_returns_to_normal() {
        // Arrange
        let mut state = make_state();
        state.input_mode = InputMode::DirectoryInput;

        // Act
        let result = handle_settings_dir_input(&mut state, KeyCode::Esc);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn dir_input_enter_returns_to_normal() {
        // Arrange
        let mut state = make_state();
        state.input_mode = InputMode::DirectoryInput;

        // Act
        let result = handle_settings_dir_input(&mut state, KeyCode::Enter);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn dir_input_backspace_pops_char() {
        // Arrange
        let mut state = make_state();
        state.settings.directory = String::from("abc");

        // Act
        let result = handle_settings_dir_input(&mut state, KeyCode::Backspace);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.settings.directory, "ab");
    }

    #[test]
    fn dir_input_char_pushes() {
        // Arrange
        let mut state = make_state();

        // Act
        let result = handle_settings_dir_input(&mut state, KeyCode::Char('x'));

        // Assert
        assert!(result.is_none());
        assert_eq!(state.settings.directory, "x");
    }

    #[test]
    fn dir_input_unknown_key_does_nothing() {
        // Arrange
        let mut state = make_state();
        let dir_before = state.settings.directory.clone();

        // Act
        let result = handle_settings_dir_input(&mut state, KeyCode::F(1));

        // Assert
        assert!(result.is_none());
        assert_eq!(state.settings.directory, dir_before);
    }

    // ── handle_confirm ──────────────────────────────────────────

    #[test]
    fn confirm_q_cancels() {
        // Arrange
        let mut state = make_state();
        state.step = WizardStep::Confirm;

        // Act
        let result = handle_confirm(&mut state, KeyCode::Char('q'), KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Cancelled));
    }

    #[test]
    fn confirm_ctrl_c_cancels() {
        // Arrange
        let mut state = make_state();
        state.step = WizardStep::Confirm;

        // Act
        let result = handle_confirm(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL);

        // Assert
        assert_eq!(result, Some(SelectorResult::Cancelled));
    }

    #[test]
    fn confirm_esc_goes_back_to_settings() {
        // Arrange
        let mut state = make_state();
        state.step = WizardStep::Confirm;
        state.confirm_count = 1;

        // Act
        let result = handle_confirm(&mut state, KeyCode::Esc, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.step, WizardStep::ConfigureSettings);
        assert_eq!(state.confirm_count, 0);
    }

    #[test]
    fn confirm_enter_once_confirms_when_no_remove() {
        // Arrange
        let mut state = make_state();
        state.step = WizardStep::Confirm;
        state.settings.remove_original = false;

        // Act
        let result = handle_confirm(&mut state, KeyCode::Enter, KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Confirmed));
    }

    #[test]
    fn confirm_enter_once_not_enough_when_remove_original() {
        // Arrange
        let mut state = make_state();
        state.step = WizardStep::Confirm;
        state.settings.remove_original = true;

        // Act
        let result = handle_confirm(&mut state, KeyCode::Enter, KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
        assert_eq!(state.confirm_count, 1);
    }

    #[test]
    fn confirm_enter_twice_confirms_when_remove_original() {
        // Arrange
        let mut state = make_state();
        state.step = WizardStep::Confirm;
        state.settings.remove_original = true;

        // Act
        handle_confirm(&mut state, KeyCode::Enter, KeyModifiers::NONE);
        let result = handle_confirm(&mut state, KeyCode::Enter, KeyModifiers::NONE);

        // Assert
        assert_eq!(result, Some(SelectorResult::Confirmed));
    }

    #[test]
    fn confirm_unknown_key_returns_none() {
        // Arrange
        let mut state = make_state();
        state.step = WizardStep::Confirm;

        // Act
        let result = handle_confirm(&mut state, KeyCode::F(1), KeyModifiers::NONE);

        // Assert
        assert!(result.is_none());
    }
}
