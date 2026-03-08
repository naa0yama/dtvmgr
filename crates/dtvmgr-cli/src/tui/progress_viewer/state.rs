//! Progress viewer TUI state management.

/// Maximum number of log lines kept in the buffer.
const MAX_LOG_LINES: usize = 500;

/// Progress viewer state.
#[allow(clippy::module_name_repetitions)]
pub struct ProgressViewerState {
    /// Current stage number (1-indexed, 0 = not started).
    pub current_stage: u8,
    /// Total number of stages.
    pub total_stages: u8,
    /// Dynamic status text for the current stage.
    pub stage_status: String,
    /// Progress within the current stage (0.0 to 1.0).
    pub stage_percent: f64,
    /// Scrolling log buffer from external command stderr.
    pub logs: Vec<String>,
    /// Whether the pipeline has finished.
    pub finished: bool,
}

impl ProgressViewerState {
    /// Create a new initial state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current_stage: 0,
            total_stages: 4,
            stage_status: String::new(),
            stage_percent: 0.0,
            logs: Vec::new(),
            finished: false,
        }
    }

    /// Push a log line, dropping oldest entries when the buffer is full.
    pub fn push_log(&mut self, line: String) {
        self.logs.push(line);
        if self.logs.len() > MAX_LOG_LINES {
            let excess = self.logs.len().saturating_sub(MAX_LOG_LINES);
            self.logs.drain(..excess);
        }
    }
}
