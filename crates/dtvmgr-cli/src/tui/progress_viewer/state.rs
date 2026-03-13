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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn push_log_single_line() {
        // Arrange
        let mut state = ProgressViewerState::new();

        // Act
        state.push_log(String::from("line 1"));

        // Assert
        assert_eq!(state.logs.len(), 1);
        assert_eq!(state.logs[0], "line 1");
    }

    #[test]
    fn push_log_fills_to_max() {
        // Arrange
        let mut state = ProgressViewerState::new();

        // Act
        for i in 0..MAX_LOG_LINES {
            state.push_log(format!("line {i}"));
        }

        // Assert
        assert_eq!(state.logs.len(), MAX_LOG_LINES);
        assert_eq!(state.logs[0], "line 0");
        assert_eq!(state.logs[MAX_LOG_LINES - 1], "line 499");
    }

    #[test]
    fn push_log_drops_oldest_on_overflow() {
        // Arrange
        let mut state = ProgressViewerState::new();
        for i in 0..MAX_LOG_LINES {
            state.push_log(format!("line {i}"));
        }

        // Act: push one more line
        state.push_log(String::from("overflow"));

        // Assert: oldest dropped, newest at end
        assert_eq!(state.logs.len(), MAX_LOG_LINES);
        assert_eq!(state.logs[0], "line 1");
        assert_eq!(state.logs[MAX_LOG_LINES - 1], "overflow");
    }
}
