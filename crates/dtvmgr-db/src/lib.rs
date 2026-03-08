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
pub use channels::{load_channel_groups, load_channels, upsert_channel_groups, upsert_channels};
#[allow(clippy::module_name_repetitions)]
pub use connection::open_db;
pub use programs::{
    delete_programs_by_tids_not_in, load_programs, load_programs_by_tids, upsert_programs,
};
pub use rusqlite::Connection;
pub use titles::{
    delete_titles_by_cat_not_in, filter_keywords, load_titles, load_titles_by_tids, parse_keywords,
    update_tmdb_last_updated, update_tmdb_mapping, update_tmdb_search_result, upsert_titles,
};
