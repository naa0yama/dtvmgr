//! Progress viewer TUI main loop.

/// Progress viewer state types.
pub mod state;
mod ui;

use std::io;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use self::state::ProgressViewerState;
use dtvmgr_jlse::progress::ProgressEvent;

/// Runs the progress viewer TUI.
///
/// Receives `ProgressEvent`s from the pipeline thread via the channel
/// and renders them in a ratatui-based TUI. Returns the pipeline result
/// when the pipeline finishes or the user quits.
///
/// # Errors
///
/// Returns an error if terminal setup, event handling, or the pipeline fails.
#[allow(clippy::module_name_repetitions)]
pub fn run_progress_viewer(
    rx: &mpsc::Receiver<ProgressEvent>,
    pipeline_handle: JoinHandle<Result<()>>,
) -> Result<()> {
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)
        .context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    let mut state = ProgressViewerState::new();

    let result = run_event_loop(&mut terminal, &mut state, rx);

    // Cleanup (always attempt even if event loop failed)
    disable_raw_mode().context("failed to disable raw mode")?;
    crossterm::execute!(io::stdout(), LeaveAlternateScreen, cursor::Show)
        .context("failed to leave alternate screen")?;

    result?;

    if state.finished {
        // Collect the pipeline result
        pipeline_handle
            .join()
            .map_err(|_| anyhow::anyhow!("pipeline thread panicked"))?
    } else {
        // User cancelled — terminate the process to kill the pipeline
        // thread and its child processes (e.g. ffmpeg). Simply dropping
        // the handle would leave child processes running as orphans.
        drop(pipeline_handle);
        #[allow(clippy::exit)]
        std::process::exit(0);
    }
}

/// Main event loop.
fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut ProgressViewerState,
    rx: &mpsc::Receiver<ProgressEvent>,
) -> Result<()> {
    loop {
        terminal
            .draw(|frame| ui::draw(frame, state))
            .context("failed to draw TUI")?;

        // Drain all pending progress events
        loop {
            match rx.try_recv() {
                Ok(evt) => match evt {
                    ProgressEvent::StageStart { stage, total, name } => {
                        state.current_stage = stage;
                        state.total_stages = total;
                        state.stage_status = format!("{name} starting");
                        state.stage_percent = 0.0;
                    }
                    ProgressEvent::StageProgress { percent, log }
                    | ProgressEvent::Encoding { percent, log } => {
                        state.stage_percent = percent;
                        state.stage_status = log;
                    }
                    ProgressEvent::Log(line) => {
                        state.push_log(line);
                    }
                    ProgressEvent::Finished => {
                        state.finished = true;
                    }
                },
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Pipeline thread exited (likely with an error)
                    // without sending Finished — mark as done so the
                    // caller can collect the thread result.
                    state.finished = true;
                    break;
                }
            }
        }

        if state.finished {
            return Ok(());
        }

        // Poll for keyboard input
        if event::poll(Duration::from_millis(50)).context("failed to poll events")?
            && let Event::Key(key) = event::read().context("failed to read event")?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}
