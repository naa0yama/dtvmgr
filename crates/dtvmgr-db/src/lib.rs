//! Database module for caching API responses.
//!
//! Uses `rusqlite` (bundled `SQLite`) to cache channel and
//! channel group data from the Syoboi Calendar API.

/// Channel cache CRUD operations.
pub mod channels;
mod connection;
mod migrations;

#[allow(clippy::module_name_repetitions)]
pub use channels::{load_channel_groups, load_channels, save_channel_groups, save_channels};
#[allow(clippy::module_name_repetitions)]
pub use connection::open_db;
