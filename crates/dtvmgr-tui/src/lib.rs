//! Terminal UI components for dtvmgr.
//!
//! Provides interactive TUI widgets built on `ratatui` + `crossterm`.

mod channel_selector;
/// Encode selector TUI.
pub mod encode_selector;
/// Shared formatting utilities.
pub mod fmt;
/// Normalize viewer TUI.
pub mod normalize_viewer;
/// Progress viewer TUI.
pub mod progress_viewer;
/// Channel selector state types.
pub mod state;
/// Title/program viewer TUI.
pub mod title_viewer;
mod ui;

pub use channel_selector::run_channel_selector;
