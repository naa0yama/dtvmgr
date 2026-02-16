//! TUI module for interactive terminal interfaces.
//!
//! Uses `ratatui` + `crossterm` for rendering.

mod channel_selector;
/// Channel selector state types.
pub mod state;
/// Title/program viewer TUI.
pub mod title_viewer;
mod ui;

pub use channel_selector::run_channel_selector;
