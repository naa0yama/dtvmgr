//! Database module for caching API responses.
//!
//! Uses `rusqlite` (bundled `SQLite`) to cache channel, title,
//! and program data from the Syoboi Calendar API.

/// Channel cache CRUD operations.
pub mod channels;
mod connection;
mod migrations;
/// Program cache CRUD operations.
pub mod programs;
/// Title cache CRUD operations.
pub mod titles;

#[allow(clippy::module_name_repetitions)]
pub use channels::{load_channel_groups, load_channels, save_channel_groups, save_channels};
#[allow(clippy::module_name_repetitions)]
pub use connection::open_db;
pub use programs::{load_programs, load_programs_by_tids, upsert_programs};
pub use titles::{load_titles, load_titles_by_tids, update_tmdb_mapping, upsert_titles};
