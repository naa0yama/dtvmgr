//! `TSDuck` command wrappers and EIT parser for MPEG-TS files.

/// External command wrappers for `TSDuck` tools.
pub mod command;
/// EIT (Event Information Table) XML parser.
pub mod eit;
/// PAT (Program Association Table) XML parser.
pub mod pat;
/// TS file seek and chunk extraction.
pub mod seek;
