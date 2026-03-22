//! Application configuration module.
//!
//! Manages TOML-based config files for user settings such as
//! selected channel IDs.

#[allow(clippy::module_inception)]
mod config;
pub mod mapping;
mod paths;

#[allow(clippy::module_name_repetitions)]
pub use config::AppConfig;
pub use mapping::load_or_fetch;
pub use paths::{resolve_config_path, resolve_data_dir};
