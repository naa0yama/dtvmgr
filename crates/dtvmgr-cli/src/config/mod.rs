//! Application configuration module.
//!
//! Manages TOML-based config files for user settings such as
//! selected channel IDs.

#[allow(clippy::module_inception)]
mod config;
mod paths;

#[allow(clippy::module_name_repetitions)]
pub use config::AppConfig;
pub use paths::resolve_config_path;
