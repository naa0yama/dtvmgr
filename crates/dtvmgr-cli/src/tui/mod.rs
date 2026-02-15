//! TUI module for interactive channel selection.
//!
//! Uses `ratatui` + `crossterm` to provide a two-pane
//! channel group / channel selector interface.

mod channel_selector;
/// Channel selector state types.
pub mod state;
mod ui;

pub use channel_selector::run_channel_selector;
