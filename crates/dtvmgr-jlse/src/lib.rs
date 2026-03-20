//! `dtvmgr-jlse` -- CM detection pipeline for Japanese TV broadcast TS files.
//!
//! This crate provides channel detection and JL parameter matching
//! functionality ported from the Node.js `join_logo_scp_trial` tool.

/// Input AVS template generation.
pub mod avs;
/// Broadcast channel detection from filenames.
pub mod channel;
/// External command wrappers for pipeline tools.
pub mod command;
/// Output file generation (AVS concatenation, chapter generation).
pub mod output;
/// JL parameter detection from channel and filename.
pub mod param;
/// Pipeline orchestration for the CM detection workflow.
pub mod pipeline;
/// EPGStation-compatible progress output.
pub mod progress;
/// Output paths, binary paths, and data paths for pipeline execution.
pub mod settings;
/// Storage statistics collection for monitoring disk usage.
pub mod storage;
/// Core type definitions.
pub mod types;
/// Pre-encode duration validation.
pub mod validate;
