//! `dtvmgr-jlse` -- CM detection pipeline for Japanese TV broadcast TS files.
//!
//! This crate provides channel detection and JL parameter matching
//! functionality ported from the Node.js `join_logo_scp_trial` tool.

/// Broadcast channel detection from filenames.
pub mod channel;
/// JL parameter detection from channel and filename.
pub mod param;
/// Core type definitions.
pub mod types;
