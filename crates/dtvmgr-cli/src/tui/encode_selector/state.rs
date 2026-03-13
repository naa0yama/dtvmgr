//! Encode selector TUI state management.

use std::collections::BTreeSet;

use ratatui::widgets::TableState;

/// Wizard step in the encode selector flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    /// Step 1: Select recordings to encode.
    SelectRecordings,
    /// Step 2: Configure encode settings.
    ConfigureSettings,
    /// Step 3: Confirm and submit.
    Confirm,
}

/// Input mode for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Normal navigation mode.
    Normal,
    /// Text input for directory field.
    DirectoryInput,
}

/// Message from the background sync task.
#[derive(Debug, Clone)]
pub enum SyncMessage {
    /// Progress update: (fetched records, total records).
    Progress {
        /// Number of records fetched so far.
        fetched: usize,
        /// Total number of records to fetch.
        total: usize,
    },
    /// Sync completed successfully.
    Complete,
}

/// Message from a background file existence check task.
#[derive(Debug, Clone)]
pub enum FileCheckMessage {
    /// Single file check result.
    Result {
        /// Recorded item ID.
        recorded_id: u64,
        /// Whether the file exists.
        exists: bool,
    },
    /// All checks for this page are complete.
    Complete,
}

/// A single running encode item for display.
#[derive(Debug, Clone, PartialEq)]
pub struct RunningEncodeItem {
    /// Program name.
    pub name: String,
    /// Encode preset mode.
    pub mode: String,
    /// Progress percentage (0-100).
    pub percent: Option<f64>,
}

/// Encode queue status for header display.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodeQueueInfo {
    /// Currently running items.
    pub running: Vec<RunningEncodeItem>,
    /// Number of waiting items.
    pub waiting_count: usize,
}

/// Message from the encode queue polling task.
#[derive(Debug, Clone)]
pub enum QueueMessage {
    /// Updated queue state.
    Update(EncodeQueueInfo),
}

/// Result of the TUI interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorResult {
    /// User confirmed and submitted encode jobs.
    Confirmed,
    /// User cancelled.
    Cancelled,
    /// User requested next page.
    PageNext,
    /// User requested previous page.
    PagePrev,
    /// User requested a refresh of file existence checks.
    Refresh,
}

/// A row representing a recorded program for display.
#[derive(Debug, Clone)]
pub struct EncodeRow {
    /// Recorded item ID.
    pub recorded_id: u64,
    /// Channel name (resolved from channel ID).
    pub channel_name: String,
    /// Program name.
    pub name: String,
    /// Start timestamp (Unix ms).
    pub start_at: u64,
    /// End timestamp (Unix ms).
    pub end_at: u64,
    /// Video resolution (e.g. "1080i").
    pub video_resolution: String,
    /// Video type (e.g. "mpeg2").
    pub video_type: String,
    /// Source TS video file ID.
    pub source_video_file_id: Option<u64>,
    /// File size in bytes.
    pub file_size: u64,
    /// Drop count.
    pub drop_cnt: u64,
    /// Error count.
    pub error_cnt: u64,
    /// Whether the item is currently recording.
    pub is_recording: bool,
    /// Whether the item is currently encoding.
    pub is_encoding: bool,
    /// Whether the source file exists on disk.
    pub file_exists: bool,
}

/// Encode settings configured in step 2.
#[derive(Debug, Clone)]
pub struct EncodeSettings {
    /// Selected encode preset name.
    pub mode: String,
    /// Index into the available presets list.
    pub preset_index: usize,
    /// Whether to save in the same directory as the source.
    pub is_save_same_directory: bool,
    /// Selected parent directory name.
    pub parent_dir: String,
    /// Index into the available parent dirs list.
    pub parent_dir_index: usize,
    /// Sub-directory name (optional).
    pub directory: String,
    /// Whether to remove the original file after encoding.
    pub remove_original: bool,
}

/// Currently focused settings field in step 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    /// Encode preset.
    Preset,
    /// Save in same directory toggle.
    SaveSameDirectory,
    /// Parent directory selection.
    ParentDir,
    /// Sub-directory text input.
    Directory,
    /// Remove original toggle.
    RemoveOriginal,
}

impl SettingsField {
    /// Returns the next field in order.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Preset => Self::SaveSameDirectory,
            Self::SaveSameDirectory => Self::ParentDir,
            Self::ParentDir => Self::Directory,
            Self::Directory => Self::RemoveOriginal,
            Self::RemoveOriginal => Self::Preset,
        }
    }

    /// Returns the previous field in order.
    #[must_use]
    pub const fn prev(self) -> Self {
        match self {
            Self::Preset => Self::RemoveOriginal,
            Self::SaveSameDirectory => Self::Preset,
            Self::ParentDir => Self::SaveSameDirectory,
            Self::Directory => Self::ParentDir,
            Self::RemoveOriginal => Self::Directory,
        }
    }
}

/// Pagination information for the encode selector.
#[derive(Debug, Clone, Copy)]
pub struct PageInfo {
    /// Current page offset (0-based, in items).
    pub offset: u64,
    /// Number of items per page.
    pub size: u64,
    /// Total number of items on the server.
    pub total: u64,
}

/// State for the encode selector TUI.
#[allow(clippy::module_name_repetitions)]
pub struct EncodeSelectorState {
    /// All recording rows.
    pub rows: Vec<EncodeRow>,
    /// Currently selected recording IDs.
    pub selected: BTreeSet<u64>,
    /// Table state (handles selection and scroll).
    pub table_state: TableState,
    /// Current wizard step.
    pub step: WizardStep,
    /// Current input mode.
    pub input_mode: InputMode,
    /// Cached filtered row indices.
    filtered_indices: Vec<usize>,
    /// Available encode presets.
    pub presets: Vec<String>,
    /// Available parent directory names.
    pub parent_dirs: Vec<String>,
    /// Encode settings.
    pub settings: EncodeSettings,
    /// Currently focused settings field.
    pub settings_field: SettingsField,
    /// Confirm step: number of Enter presses received.
    pub confirm_count: u8,
    /// Pagination information.
    pub page: PageInfo,
    /// Whether to hide rows that cannot be encoded (no file or no source video).
    pub hide_unavailable: bool,
    /// Cached count of unavailable rows (recomputed in `rebuild_filter`).
    hidden_count: usize,
    /// Background sync progress: `(fetched, total)`.
    pub sync_progress: Option<(usize, usize)>,
    /// Background file check progress: `(checked, total)`.
    pub file_check_progress: Option<(usize, usize)>,
    /// Encode queue status for header display.
    pub encode_queue: Option<EncodeQueueInfo>,
}

impl EncodeSelectorState {
    /// Creates a new state.
    #[must_use]
    pub fn new(
        rows: Vec<EncodeRow>,
        presets: Vec<String>,
        parent_dirs: Vec<String>,
        default_preset: Option<&str>,
        default_directory: Option<&str>,
        page: PageInfo,
    ) -> Self {
        let filtered_indices: Vec<usize> = (0..rows.len()).collect();
        let hidden_count = rows
            .iter()
            .filter(|row| !row.file_exists || row.source_video_file_id.is_none())
            .count();

        let preset_index = default_preset
            .and_then(|p| presets.iter().position(|n| n == p))
            .unwrap_or(0);
        let mode = presets
            .get(preset_index)
            .cloned()
            .unwrap_or_else(|| String::from("H.264"));
        let parent_dir = parent_dirs
            .first()
            .cloned()
            .unwrap_or_else(|| String::from("recorded"));

        let mut table_state = TableState::default();
        if !rows.is_empty() {
            table_state.select(Some(0));
        }

        Self {
            rows,
            selected: BTreeSet::new(),
            table_state,
            step: WizardStep::SelectRecordings,
            input_mode: InputMode::Normal,
            filtered_indices,
            presets,
            parent_dirs,
            settings: EncodeSettings {
                mode,
                preset_index,
                is_save_same_directory: false,
                parent_dir,
                parent_dir_index: 0,
                directory: default_directory.map(String::from).unwrap_or_default(),
                remove_original: false,
            },
            settings_field: SettingsField::Preset,
            confirm_count: 0,
            page,
            hide_unavailable: false,
            hidden_count,
            sync_progress: None,
            file_check_progress: None,
            encode_queue: None,
        }
    }

    /// Returns the filtered row indices.
    #[must_use]
    pub fn filtered_indices(&self) -> &[usize] {
        &self.filtered_indices
    }

    /// Rebuilds filtered indices from the `hide_unavailable` flag.
    pub fn rebuild_filter(&mut self) {
        let mut unavailable_count: usize = 0;
        self.filtered_indices = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                let is_unavailable = !row.file_exists || row.source_video_file_id.is_none();
                if is_unavailable {
                    unavailable_count = unavailable_count.saturating_add(1);
                }
                if self.hide_unavailable && is_unavailable {
                    return false;
                }
                true
            })
            .map(|(i, _)| i)
            .collect();
        self.hidden_count = unavailable_count;
        // Reset cursor
        if self.filtered_indices.is_empty() {
            self.table_state.select(None);
        } else {
            self.table_state.select(Some(0));
        }
    }

    /// Toggles the hide-unavailable filter and rebuilds.
    pub fn toggle_hide_unavailable(&mut self) {
        self.hide_unavailable = !self.hide_unavailable;
        self.rebuild_filter();
    }

    /// Returns the number of unavailable rows (cached from last `rebuild_filter`).
    #[must_use]
    pub const fn hidden_count(&self) -> usize {
        self.hidden_count
    }

    /// Updates `file_exists` for a row by `recorded_id` without rebuilding filter.
    ///
    /// Call `rebuild_filter()` after processing a batch of updates.
    pub fn update_file_exists(&mut self, recorded_id: u64, exists: bool) {
        if let Some(row) = self.rows.iter_mut().find(|r| r.recorded_id == recorded_id) {
            row.file_exists = exists;
        }
    }

    /// Moves cursor up (clamped at top).
    pub fn move_up(&mut self) {
        let len = self.filtered_indices.len();
        if len == 0 {
            return;
        }
        let i = self
            .table_state
            .selected()
            .map_or(0, |i| i.saturating_sub(1));
        self.table_state.select(Some(i));
    }

    /// Moves cursor down (clamped at bottom).
    pub fn move_down(&mut self) {
        let len = self.filtered_indices.len();
        if len == 0 {
            return;
        }
        let i = self
            .table_state
            .selected()
            .map_or(0, |i| i.saturating_add(1).min(len.saturating_sub(1)));
        self.table_state.select(Some(i));
    }

    /// Toggles selection for the current row.
    pub fn toggle_current(&mut self) {
        if let Some(cursor) = self.table_state.selected()
            && let Some(&row_idx) = self.filtered_indices.get(cursor)
            && let Some(row) = self.rows.get(row_idx)
            && row.file_exists
            && row.source_video_file_id.is_some()
        {
            let id = row.recorded_id;
            if self.selected.contains(&id) {
                self.selected.remove(&id);
            } else {
                self.selected.insert(id);
            }
        }
    }

    /// Selects all visible (filtered) rows that are selectable.
    pub fn select_all(&mut self) {
        for &idx in &self.filtered_indices {
            if let Some(row) = self.rows.get(idx)
                && row.file_exists
                && row.source_video_file_id.is_some()
            {
                self.selected.insert(row.recorded_id);
            }
        }
    }

    /// Deselects all visible (filtered) rows.
    pub fn deselect_all(&mut self) {
        for &idx in &self.filtered_indices {
            if let Some(row) = self.rows.get(idx) {
                self.selected.remove(&row.recorded_id);
            }
        }
    }

    /// Cycles the preset to the next one.
    pub fn next_preset(&mut self) {
        if self.presets.is_empty() {
            return;
        }
        self.settings.preset_index = (self.settings.preset_index.saturating_add(1))
            .checked_rem(self.presets.len())
            .unwrap_or(0);
        if let Some(name) = self.presets.get(self.settings.preset_index) {
            self.settings.mode = name.clone();
        }
    }

    /// Cycles the preset to the previous one.
    pub fn prev_preset(&mut self) {
        if self.presets.is_empty() {
            return;
        }
        self.settings.preset_index = if self.settings.preset_index == 0 {
            self.presets.len().saturating_sub(1)
        } else {
            self.settings.preset_index.saturating_sub(1)
        };
        if let Some(name) = self.presets.get(self.settings.preset_index) {
            self.settings.mode = name.clone();
        }
    }

    /// Cycles the parent directory to the next one.
    pub fn next_parent_dir(&mut self) {
        if self.parent_dirs.is_empty() {
            return;
        }
        self.settings.parent_dir_index = (self.settings.parent_dir_index.saturating_add(1))
            .checked_rem(self.parent_dirs.len())
            .unwrap_or(0);
        if let Some(name) = self.parent_dirs.get(self.settings.parent_dir_index) {
            self.settings.parent_dir = name.clone();
        }
    }

    /// Cycles the parent directory to the previous one.
    pub fn prev_parent_dir(&mut self) {
        if self.parent_dirs.is_empty() {
            return;
        }
        self.settings.parent_dir_index = if self.settings.parent_dir_index == 0 {
            self.parent_dirs.len().saturating_sub(1)
        } else {
            self.settings.parent_dir_index.saturating_sub(1)
        };
        if let Some(name) = self.parent_dirs.get(self.settings.parent_dir_index) {
            self.settings.parent_dir = name.clone();
        }
    }

    /// Returns the number of required Enter presses to confirm.
    #[must_use]
    pub const fn required_confirms(&self) -> u8 {
        if self.settings.remove_original { 2 } else { 1 }
    }

    /// Whether there is a next page.
    #[must_use]
    pub const fn has_next_page(&self) -> bool {
        self.page.offset.saturating_add(self.page.size) < self.page.total
    }

    /// Whether there is a previous page.
    #[must_use]
    pub const fn has_prev_page(&self) -> bool {
        self.page.offset > 0
    }

    /// Current 1-based page number.
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub const fn current_page(&self) -> u64 {
        if self.page.size == 0 {
            return 1;
        }
        // Division is safe: page.size != 0 is guaranteed by the guard above.
        self.page.offset / self.page.size + 1
    }

    /// Total number of pages.
    #[must_use]
    #[allow(clippy::arithmetic_side_effects, clippy::manual_div_ceil)]
    pub const fn total_pages(&self) -> u64 {
        if self.page.size == 0 {
            return 1;
        }
        // Division is safe: page.size != 0 is guaranteed by the guard above.
        // Using manual div_ceil because div_ceil() is not const-stable.
        (self.page.total + self.page.size - 1) / self.page.size
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// Creates a minimal state with `n` rows for testing.
    fn make_state(n: usize) -> EncodeSelectorState {
        let rows: Vec<EncodeRow> = (0..n)
            .map(|i| {
                #[allow(clippy::as_conversions)]
                let id = i as u64;
                EncodeRow {
                    recorded_id: id,
                    channel_name: String::from("ch"),
                    name: format!("row{i}"),
                    start_at: 0,
                    end_at: 0,
                    video_resolution: String::new(),
                    video_type: String::new(),
                    source_video_file_id: Some(id),
                    file_size: 0,
                    drop_cnt: 0,
                    error_cnt: 0,
                    is_recording: false,
                    is_encoding: false,
                    file_exists: true,
                }
            })
            .collect();
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 100,
        };
        EncodeSelectorState::new(rows, vec![], vec![], None, None, page)
    }

    #[test]
    fn move_up_does_not_wrap_at_top() {
        let mut state = make_state(5);
        assert_eq!(state.table_state.selected(), Some(0));

        state.move_up();
        assert_eq!(state.table_state.selected(), Some(0));
    }

    #[test]
    fn move_down_does_not_wrap_at_bottom() {
        let mut state = make_state(5);
        // Move to last item
        for _ in 0..10 {
            state.move_down();
        }
        assert_eq!(state.table_state.selected(), Some(4));

        // Should stay at last
        state.move_down();
        assert_eq!(state.table_state.selected(), Some(4));
    }

    #[test]
    fn move_up_down_normal_navigation() {
        let mut state = make_state(5);
        state.move_down();
        assert_eq!(state.table_state.selected(), Some(1));
        state.move_down();
        assert_eq!(state.table_state.selected(), Some(2));
        state.move_up();
        assert_eq!(state.table_state.selected(), Some(1));
    }

    #[test]
    fn has_next_page_true_when_more_items() {
        let state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 0,
                size: 10,
                total: 30,
            },
        );
        assert!(state.has_next_page());
    }

    #[test]
    fn has_next_page_false_on_last_page() {
        let state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 20,
                size: 10,
                total: 30,
            },
        );
        assert!(!state.has_next_page());
    }

    #[test]
    fn has_prev_page_false_on_first_page() {
        let state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 0,
                size: 10,
                total: 30,
            },
        );
        assert!(!state.has_prev_page());
    }

    #[test]
    fn has_prev_page_true_after_first_page() {
        let state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 10,
                size: 10,
                total: 30,
            },
        );
        assert!(state.has_prev_page());
    }

    #[test]
    fn current_page_and_total_pages() {
        let state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 0,
                size: 10,
                total: 25,
            },
        );
        assert_eq!(state.current_page(), 1);
        assert_eq!(state.total_pages(), 3);

        let state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 10,
                size: 10,
                total: 25,
            },
        );
        assert_eq!(state.current_page(), 2);

        let state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 20,
                size: 10,
                total: 25,
            },
        );
        assert_eq!(state.current_page(), 3);
    }

    #[test]
    fn pagination_edge_case_zero_page_size() {
        let state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 0,
                size: 0,
                total: 0,
            },
        );
        assert_eq!(state.current_page(), 1);
        assert_eq!(state.total_pages(), 1);
    }

    /// Creates a state with mixed availability for filter tests.
    fn make_mixed_state() -> EncodeSelectorState {
        let rows = vec![
            EncodeRow {
                recorded_id: 0,
                channel_name: String::from("ch"),
                name: String::from("available"),
                start_at: 0,
                end_at: 0,
                video_resolution: String::new(),
                video_type: String::new(),
                source_video_file_id: Some(0),
                file_size: 0,
                drop_cnt: 0,
                error_cnt: 0,
                is_recording: false,
                is_encoding: false,
                file_exists: true,
            },
            EncodeRow {
                recorded_id: 1,
                channel_name: String::from("ch"),
                name: String::from("no_file"),
                start_at: 0,
                end_at: 0,
                video_resolution: String::new(),
                video_type: String::new(),
                source_video_file_id: Some(1),
                file_size: 0,
                drop_cnt: 0,
                error_cnt: 0,
                is_recording: false,
                is_encoding: false,
                file_exists: false,
            },
            EncodeRow {
                recorded_id: 2,
                channel_name: String::from("ch"),
                name: String::from("no_source"),
                start_at: 0,
                end_at: 0,
                video_resolution: String::new(),
                video_type: String::new(),
                source_video_file_id: None,
                file_size: 0,
                drop_cnt: 0,
                error_cnt: 0,
                is_recording: false,
                is_encoding: false,
                file_exists: true,
            },
        ];
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 3,
        };
        EncodeSelectorState::new(rows, vec![], vec![], None, None, page)
    }

    #[test]
    fn toggle_hide_unavailable_filters_rows() {
        // Arrange
        let mut state = make_mixed_state();
        assert_eq!(state.filtered_indices().len(), 3);

        // Act
        state.toggle_hide_unavailable();

        // Assert: only the available row remains
        assert_eq!(state.filtered_indices().len(), 1);
        assert_eq!(state.filtered_indices().first().copied(), Some(0));
    }

    #[test]
    fn toggle_hide_unavailable_off_restores_all() {
        // Arrange
        let mut state = make_mixed_state();
        state.toggle_hide_unavailable();
        assert_eq!(state.filtered_indices().len(), 1);

        // Act
        state.toggle_hide_unavailable();

        // Assert
        assert_eq!(state.filtered_indices().len(), 3);
    }

    #[test]
    fn hidden_count_returns_unavailable_rows() {
        let state = make_mixed_state();
        assert_eq!(state.hidden_count(), 2);
    }

    #[test]
    fn update_file_exists_updates_row() {
        // Arrange
        let mut state = make_mixed_state();
        assert!(!state.rows.get(1).unwrap().file_exists); // no_file row

        // Act: mark file as existing
        state.update_file_exists(1, true);

        // Assert: row is updated
        assert!(state.rows.get(1).unwrap().file_exists);
        // hidden_count is stale until rebuild_filter is called
        assert_eq!(state.hidden_count(), 2);

        // Act: rebuild filter to refresh counts
        state.rebuild_filter();

        // Assert: hidden_count now reflects the change
        assert_eq!(state.hidden_count(), 1);
    }
}
