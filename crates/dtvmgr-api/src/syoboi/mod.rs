//! Syoboi Calendar API client module.
//!
//! Handles HTTP requests to the Syoboi Calendar `db.php` endpoint
//! and retrieves title, program, and channel data.

mod api;
mod client;
mod params;
mod rate_limiter;
mod types;
mod util;
pub(crate) mod xml;

#[allow(clippy::module_name_repetitions)]
pub use api::{LocalSyoboiApi, SyoboiApi};
#[allow(clippy::module_name_repetitions)]
pub use client::{SyoboiClient, SyoboiClientBuilder};
pub use params::{ProgLookupParams, TimeRange};
#[allow(clippy::module_name_repetitions)]
pub use types::{SyoboiChannel, SyoboiChannelGroup, SyoboiProgram, SyoboiTitle};
pub use util::{lookup_all_programs, parse_sub_titles};
