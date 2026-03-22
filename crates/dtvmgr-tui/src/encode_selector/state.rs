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

/// Global worker progress, shared via watch channel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileCheckWorkerProgress {
    /// Batches waiting in queue (not including current).
    pub pending: usize,
    /// Current batch progress, if actively processing.
    pub checking: Option<(usize, usize)>,
}

impl FileCheckWorkerProgress {
    /// Returns `true` when the worker is processing or has queued batches.
    #[must_use]
    pub const fn is_active(self) -> bool {
        self.checking.is_some() || self.pending > 0
    }
}

/// A batch of file existence checks to enqueue to the worker.
#[allow(missing_debug_implementations)]
pub struct FileCheckRequest {
    /// Files to check: `(video_file_id, recorded_id)`.
    pub files: Vec<(i64, i64)>,
    /// Channel to send results back to the current page's TUI.
    pub result_tx: std::sync::mpsc::Sender<FileCheckMessage>,
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
    /// Recorded item ID (for matching against the recording list).
    pub recorded_id: u64,
    /// Program name.
    pub name: String,
    /// Encode preset mode.
    pub mode: String,
    /// Progress ratio (0.0–1.0).
    pub percent: Option<f64>,
    /// Encoder log output (e.g. current step or progress line).
    pub log: Option<String>,
}

/// Encode queue status for header display and row status matching.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodeQueueInfo {
    /// Currently running items.
    pub running: Vec<RunningEncodeItem>,
    /// Number of waiting items.
    pub waiting_count: usize,
    /// Recorded IDs of items waiting in the queue.
    pub waiting_ids: BTreeSet<u64>,
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
    /// Video file types (deduplicated, e.g. "ts", "ts,enc").
    pub file_types: String,
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

/// A storage directory entry for the stats widget.
#[derive(Debug, Clone)]
pub struct StorageDirEntry {
    /// Directory display name (e.g. "recorded", "encoded").
    pub name: String,
    /// Filesystem path (may be invalid if `EPGStation` returned name-only).
    pub path: String,
    /// Whether this entry is visible in the widget.
    pub visible: bool,
    /// Collected stats snapshot (`None` if path is inaccessible).
    pub stats: Option<StorageStatsSnapshot>,
}

/// Snapshot of storage statistics for display.
#[derive(Debug, Clone)]
pub struct StorageStatsSnapshot {
    /// Total filesystem capacity in bytes.
    pub total_bytes: u64,
    /// Sum of file sizes in this directory.
    pub used_bytes: u64,
    /// Directory usage ratio against filesystem capacity.
    pub usage_ratio: f64,
    /// Number of regular files in the directory.
    pub file_count: u64,
}

/// Message from the background storage stats polling task.
#[derive(Debug, Clone)]
pub enum StorageMessage {
    /// Updated storage stats for all directories.
    Update(Vec<(String, Option<StorageStatsSnapshot>)>),
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
#[allow(clippy::module_name_repetitions, missing_debug_implementations)]
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
    /// Whether to hide rows already in the encode queue.
    pub hide_queued: bool,
    /// Cached count of unavailable rows (recomputed in `rebuild_filter`).
    hidden_count: usize,
    /// Background sync progress: `(fetched, total)`.
    pub sync_progress: Option<(usize, usize)>,
    /// Background file check worker progress (global, from watch channel).
    pub file_check_progress: Option<FileCheckWorkerProgress>,
    /// Encode queue status for header display.
    pub encode_queue: Option<EncodeQueueInfo>,
    /// Storage directory entries for the stats widget.
    pub storage_dirs: Vec<StorageDirEntry>,
    /// Visible table rows (updated by the renderer each frame).
    pub visible_table_rows: usize,
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
        recorded_dirs: Vec<(String, String)>,
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
            hide_queued: false,
            hidden_count,
            sync_progress: None,
            file_check_progress: None,
            encode_queue: None,
            visible_table_rows: 20,
            storage_dirs: recorded_dirs
                .into_iter()
                .map(|(name, path)| StorageDirEntry {
                    name,
                    path,
                    visible: true,
                    stats: None,
                })
                .collect(),
        }
    }

    /// Returns the filtered row indices.
    #[must_use]
    pub fn filtered_indices(&self) -> &[usize] {
        &self.filtered_indices
    }

    /// Rebuilds filtered indices from filter flags.
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
                if self.hide_queued && self.is_in_encode_queue(row.recorded_id) {
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

    /// Toggles the hide-queued filter and rebuilds.
    pub fn toggle_hide_queued(&mut self) {
        self.hide_queued = !self.hide_queued;
        self.rebuild_filter();
    }

    /// Returns whether a recorded ID is in the encode queue (running or waiting).
    #[must_use]
    pub fn is_in_encode_queue(&self, recorded_id: u64) -> bool {
        self.encode_queue.as_ref().is_some_and(|q| {
            q.waiting_ids.contains(&recorded_id)
                || q.running.iter().any(|r| r.recorded_id == recorded_id)
        })
    }

    /// Returns the encode queue status label for a recorded ID.
    #[must_use]
    pub fn encode_queue_status(&self, recorded_id: u64) -> &'static str {
        let Some(ref q) = self.encode_queue else {
            return "";
        };
        if q.running.iter().any(|r| r.recorded_id == recorded_id) {
            "run"
        } else if q.waiting_ids.contains(&recorded_id) {
            "wait"
        } else {
            ""
        }
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

    /// Moves cursor up by `n` rows (clamped at top).
    pub fn page_up(&mut self, n: usize) {
        let len = self.filtered_indices.len();
        if len == 0 {
            return;
        }
        let i = self
            .table_state
            .selected()
            .map_or(0, |i| i.saturating_sub(n));
        self.table_state.select(Some(i));
    }

    /// Moves cursor down by `n` rows (clamped at bottom).
    pub fn page_down(&mut self, n: usize) {
        let len = self.filtered_indices.len();
        if len == 0 {
            return;
        }
        let i = self
            .table_state
            .selected()
            .map_or(0, |i| i.saturating_add(n).min(len.saturating_sub(1)));
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

    /// Toggles visibility of a storage directory by index.
    pub fn toggle_storage_dir(&mut self, index: usize) {
        if let Some(entry) = self.storage_dirs.get_mut(index) {
            entry.visible = !entry.visible;
        }
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
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

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
                    file_types: String::new(),
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
        EncodeSelectorState::new(rows, vec![], vec![], None, None, page, vec![])
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
            vec![],
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
            vec![],
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
            vec![],
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
            vec![],
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
            vec![],
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
            vec![],
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
            vec![],
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
            vec![],
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
                file_types: String::new(),
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
                file_types: String::new(),
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
                file_types: String::new(),
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
        EncodeSelectorState::new(rows, vec![], vec![], None, None, page, vec![])
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

    // ── FileCheckWorkerProgress::is_active ───────────────────────

    #[test]
    fn is_active_idle() {
        let p = FileCheckWorkerProgress {
            pending: 0,
            checking: None,
        };
        assert!(!p.is_active());
    }

    #[test]
    fn is_active_checking() {
        let p = FileCheckWorkerProgress {
            pending: 0,
            checking: Some((3, 10)),
        };
        assert!(p.is_active());
    }

    #[test]
    fn is_active_pending() {
        let p = FileCheckWorkerProgress {
            pending: 2,
            checking: None,
        };
        assert!(p.is_active());
    }

    // ── SettingsField::next / prev ───────────────────────────────

    #[test]
    fn settings_field_next_cycle() {
        assert_eq!(
            SettingsField::Preset.next(),
            SettingsField::SaveSameDirectory
        );
        assert_eq!(
            SettingsField::SaveSameDirectory.next(),
            SettingsField::ParentDir
        );
        assert_eq!(SettingsField::ParentDir.next(), SettingsField::Directory);
        assert_eq!(
            SettingsField::Directory.next(),
            SettingsField::RemoveOriginal
        );
        // Wrap around
        assert_eq!(SettingsField::RemoveOriginal.next(), SettingsField::Preset);
    }

    #[test]
    fn settings_field_prev_cycle() {
        assert_eq!(SettingsField::Preset.prev(), SettingsField::RemoveOriginal);
        assert_eq!(
            SettingsField::SaveSameDirectory.prev(),
            SettingsField::Preset
        );
        assert_eq!(
            SettingsField::ParentDir.prev(),
            SettingsField::SaveSameDirectory
        );
        assert_eq!(SettingsField::Directory.prev(), SettingsField::ParentDir);
        assert_eq!(
            SettingsField::RemoveOriginal.prev(),
            SettingsField::Directory
        );
    }

    // ── required_confirms ────────────────────────────────────────

    #[test]
    fn required_confirms_without_remove() {
        let state = make_state(1);
        assert_eq!(state.required_confirms(), 1);
    }

    #[test]
    fn required_confirms_with_remove() {
        let mut state = make_state(1);
        state.settings.remove_original = true;
        assert_eq!(state.required_confirms(), 2);
    }

    // ── select_all / deselect_all ────────────────────────────────

    #[test]
    fn select_all_selects_available_rows() {
        // Arrange: mixed state with 1 available, 2 unavailable
        let mut state = make_mixed_state();

        // Act
        state.select_all();

        // Assert: only the available row (recorded_id=0) is selected
        assert_eq!(state.selected.len(), 1);
        assert!(state.selected.contains(&0));
    }

    #[test]
    fn deselect_all_clears_selection() {
        // Arrange
        let mut state = make_mixed_state();
        state.select_all();
        assert!(!state.selected.is_empty());

        // Act
        state.deselect_all();

        // Assert
        assert!(state.selected.is_empty());
    }

    // ── next_preset / prev_preset ────────────────────────────────

    fn make_state_with_presets() -> EncodeSelectorState {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 0,
        };
        EncodeSelectorState::new(
            vec![],
            vec![
                String::from("H.264"),
                String::from("H.265"),
                String::from("AV1"),
            ],
            vec![],
            None,
            None,
            page,
            vec![],
        )
    }

    #[test]
    fn next_preset_cycles_forward() {
        let mut state = make_state_with_presets();
        assert_eq!(state.settings.mode, "H.264");

        state.next_preset();
        assert_eq!(state.settings.mode, "H.265");

        state.next_preset();
        assert_eq!(state.settings.mode, "AV1");

        // Wrap around
        state.next_preset();
        assert_eq!(state.settings.mode, "H.264");
    }

    #[test]
    fn prev_preset_cycles_backward() {
        let mut state = make_state_with_presets();
        assert_eq!(state.settings.mode, "H.264");

        // Wrap around from first to last
        state.prev_preset();
        assert_eq!(state.settings.mode, "AV1");

        state.prev_preset();
        assert_eq!(state.settings.mode, "H.265");

        state.prev_preset();
        assert_eq!(state.settings.mode, "H.264");
    }

    #[test]
    fn next_preset_empty_noop() {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 0,
        };
        let mut state = EncodeSelectorState::new(vec![], vec![], vec![], None, None, page, vec![]);

        // Act: should not panic
        state.next_preset();
        assert_eq!(state.settings.mode, "H.264"); // default
    }

    #[test]
    fn prev_preset_empty_noop() {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 0,
        };
        let mut state = EncodeSelectorState::new(vec![], vec![], vec![], None, None, page, vec![]);

        state.prev_preset();
        assert_eq!(state.settings.mode, "H.264"); // default
    }

    // ── next_parent_dir / prev_parent_dir ────────────────────────

    fn make_state_with_parent_dirs() -> EncodeSelectorState {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 0,
        };
        EncodeSelectorState::new(
            vec![],
            vec![],
            vec![
                String::from("recorded"),
                String::from("archive"),
                String::from("tmp"),
            ],
            None,
            None,
            page,
            vec![],
        )
    }

    #[test]
    fn next_parent_dir_cycles_forward() {
        let mut state = make_state_with_parent_dirs();
        assert_eq!(state.settings.parent_dir, "recorded");

        state.next_parent_dir();
        assert_eq!(state.settings.parent_dir, "archive");

        state.next_parent_dir();
        assert_eq!(state.settings.parent_dir, "tmp");

        // Wrap around
        state.next_parent_dir();
        assert_eq!(state.settings.parent_dir, "recorded");
    }

    #[test]
    fn prev_parent_dir_cycles_backward() {
        let mut state = make_state_with_parent_dirs();
        assert_eq!(state.settings.parent_dir, "recorded");

        // Wrap around from first to last
        state.prev_parent_dir();
        assert_eq!(state.settings.parent_dir, "tmp");

        state.prev_parent_dir();
        assert_eq!(state.settings.parent_dir, "archive");

        state.prev_parent_dir();
        assert_eq!(state.settings.parent_dir, "recorded");
    }

    #[test]
    fn next_parent_dir_empty_noop() {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 0,
        };
        let mut state = EncodeSelectorState::new(vec![], vec![], vec![], None, None, page, vec![]);

        state.next_parent_dir();
        assert_eq!(state.settings.parent_dir, "recorded"); // default
    }

    #[test]
    fn prev_parent_dir_empty_noop() {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 0,
        };
        let mut state = EncodeSelectorState::new(vec![], vec![], vec![], None, None, page, vec![]);

        state.prev_parent_dir();
        assert_eq!(state.settings.parent_dir, "recorded"); // default
    }

    // ── new constructor branches ─────────────────────────────────

    #[test]
    fn new_with_default_preset() {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 0,
        };
        let state = EncodeSelectorState::new(
            vec![],
            vec![
                String::from("H.264"),
                String::from("H.265"),
                String::from("AV1"),
            ],
            vec![],
            Some("H.265"),
            None,
            page,
            vec![],
        );
        assert_eq!(state.settings.mode, "H.265");
        assert_eq!(state.settings.preset_index, 1);
    }

    #[test]
    fn new_with_default_directory() {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 0,
        };
        let state =
            EncodeSelectorState::new(vec![], vec![], vec![], None, Some("subdir"), page, vec![]);
        assert_eq!(state.settings.directory, "subdir");
    }

    // ── move_up / move_down on empty state ────────────────────────

    #[test]
    fn move_up_on_empty_state_is_noop() {
        let mut state = make_state(0);
        assert_eq!(state.table_state.selected(), None);

        state.move_up();
        assert_eq!(state.table_state.selected(), None);
    }

    #[test]
    fn move_down_on_empty_state_is_noop() {
        let mut state = make_state(0);
        assert_eq!(state.table_state.selected(), None);

        state.move_down();
        assert_eq!(state.table_state.selected(), None);
    }

    // ── page_up / page_down ───────────────────────────────────────

    #[test]
    fn page_up_moves_by_n() {
        let mut state = make_state(10);
        // Move cursor to row 7
        for _ in 0..7 {
            state.move_down();
        }
        assert_eq!(state.table_state.selected(), Some(7));

        // Page up by 3
        state.page_up(3);
        assert_eq!(state.table_state.selected(), Some(4));
    }

    #[test]
    fn page_up_clamps_at_top() {
        let mut state = make_state(5);
        state.move_down(); // cursor at 1
        state.page_up(10); // should clamp at 0
        assert_eq!(state.table_state.selected(), Some(0));
    }

    #[test]
    fn page_up_on_empty_is_noop() {
        let mut state = make_state(0);
        state.page_up(5);
        assert_eq!(state.table_state.selected(), None);
    }

    #[test]
    fn page_down_moves_by_n() {
        let mut state = make_state(10);
        assert_eq!(state.table_state.selected(), Some(0));

        state.page_down(4);
        assert_eq!(state.table_state.selected(), Some(4));
    }

    #[test]
    fn page_down_clamps_at_bottom() {
        let mut state = make_state(5);
        state.page_down(100);
        assert_eq!(state.table_state.selected(), Some(4));
    }

    #[test]
    fn page_down_on_empty_is_noop() {
        let mut state = make_state(0);
        state.page_down(5);
        assert_eq!(state.table_state.selected(), None);
    }

    // ── toggle_current edge cases ─────────────────────────────────

    #[test]
    fn toggle_current_selects_and_deselects() {
        let mut state = make_state(3);
        assert!(state.selected.is_empty());

        // Select row 0
        state.toggle_current();
        assert!(state.selected.contains(&0));

        // Deselect row 0
        state.toggle_current();
        assert!(!state.selected.contains(&0));
    }

    #[test]
    fn toggle_current_unavailable_row_is_noop() {
        let mut state = make_mixed_state();
        // Move cursor to row 1 (file_exists=false)
        state.move_down();
        assert_eq!(state.table_state.selected(), Some(1));

        state.toggle_current();
        assert!(state.selected.is_empty());
    }

    #[test]
    fn toggle_current_no_source_video_is_noop() {
        let mut state = make_mixed_state();
        // Move cursor to row 2 (source_video_file_id=None)
        state.move_down();
        state.move_down();
        assert_eq!(state.table_state.selected(), Some(2));

        state.toggle_current();
        assert!(state.selected.is_empty());
    }

    // ── toggle_storage_dir ────────────────────────────────────────

    #[test]
    fn toggle_storage_dir_toggles_visibility() {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 0,
        };
        let mut state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            page,
            vec![
                (String::from("recorded"), String::from("/data/recorded")),
                (String::from("encoded"), String::from("/data/encoded")),
            ],
        );

        assert!(state.storage_dirs[0].visible);
        state.toggle_storage_dir(0);
        assert!(!state.storage_dirs[0].visible);
        state.toggle_storage_dir(0);
        assert!(state.storage_dirs[0].visible);
    }

    #[test]
    fn toggle_storage_dir_out_of_bounds_is_noop() {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 0,
        };
        let mut state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            page,
            vec![(String::from("recorded"), String::from("/data/recorded"))],
        );

        // Should not panic
        state.toggle_storage_dir(99);
        assert!(state.storage_dirs[0].visible);
    }

    // ── encode_queue_status / is_in_encode_queue ──────────────────

    #[test]
    fn encode_queue_status_no_queue() {
        let state = make_state(3);
        assert_eq!(state.encode_queue_status(0), "");
        assert!(!state.is_in_encode_queue(0));
    }

    #[test]
    fn encode_queue_status_running() {
        let mut state = make_state(3);
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![RunningEncodeItem {
                recorded_id: 1,
                name: String::from("test"),
                mode: String::from("H.264"),
                percent: Some(0.5),
                log: None,
            }],
            waiting_count: 0,
            waiting_ids: BTreeSet::new(),
        });

        assert_eq!(state.encode_queue_status(1), "run");
        assert!(state.is_in_encode_queue(1));
        assert_eq!(state.encode_queue_status(0), "");
        assert!(!state.is_in_encode_queue(0));
    }

    #[test]
    fn encode_queue_status_waiting() {
        let mut state = make_state(3);
        let mut waiting_ids = BTreeSet::new();
        waiting_ids.insert(2);
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![],
            waiting_count: 1,
            waiting_ids,
        });

        assert_eq!(state.encode_queue_status(2), "wait");
        assert!(state.is_in_encode_queue(2));
        assert_eq!(state.encode_queue_status(0), "");
    }

    // ── toggle_hide_queued ────────────────────────────────────────

    #[test]
    fn toggle_hide_queued_filters_queued_rows() {
        let mut state = make_state(3);
        let mut waiting_ids = BTreeSet::new();
        waiting_ids.insert(1);
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![RunningEncodeItem {
                recorded_id: 0,
                name: String::from("test"),
                mode: String::from("H.264"),
                percent: None,
                log: None,
            }],
            waiting_count: 1,
            waiting_ids,
        });

        assert_eq!(state.filtered_indices().len(), 3);

        // Act — hide queued items
        state.toggle_hide_queued();

        // Assert — rows 0 (running) and 1 (waiting) are hidden
        assert_eq!(state.filtered_indices().len(), 1);
        assert_eq!(state.filtered_indices()[0], 2);

        // Act — show again
        state.toggle_hide_queued();
        assert_eq!(state.filtered_indices().len(), 3);
    }

    // ── update_file_exists with non-existent ID ───────────────────

    #[test]
    fn update_file_exists_nonexistent_id_is_noop() {
        let mut state = make_state(3);
        // Should not panic or change anything
        state.update_file_exists(999, true);
        assert_eq!(state.rows.len(), 3);
    }

    // ── new with recorded_dirs ────────────────────────────────────

    #[test]
    fn new_populates_storage_dirs() {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 0,
        };
        let state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            page,
            vec![
                (String::from("recorded"), String::from("/data/recorded")),
                (String::from("encoded"), String::from("/data/encoded")),
            ],
        );

        assert_eq!(state.storage_dirs.len(), 2);
        assert_eq!(state.storage_dirs[0].name, "recorded");
        assert_eq!(state.storage_dirs[0].path, "/data/recorded");
        assert!(state.storage_dirs[0].visible);
        assert!(state.storage_dirs[0].stats.is_none());
        assert_eq!(state.storage_dirs[1].name, "encoded");
    }

    // ── rebuild_filter resets cursor ──────────────────────────────

    #[test]
    fn rebuild_filter_resets_cursor_when_all_filtered_out() {
        let mut state = make_mixed_state();
        // Select cursor at row 2
        state.move_down();
        state.move_down();
        assert_eq!(state.table_state.selected(), Some(2));

        // Hide unavailable — only row 0 remains
        state.toggle_hide_unavailable();
        // Cursor should be reset to 0
        assert_eq!(state.table_state.selected(), Some(0));

        // Now mark last available row as unavailable too
        state.rows[0].file_exists = false;
        state.rebuild_filter();
        // All filtered out — cursor is None
        assert_eq!(state.table_state.selected(), None);
        assert!(state.filtered_indices().is_empty());
    }

    // ── new with non-matching default_preset ──────────────────────

    #[test]
    fn new_with_nonexistent_default_preset_falls_back() {
        let page = PageInfo {
            offset: 0,
            size: 10,
            total: 0,
        };
        let state = EncodeSelectorState::new(
            vec![],
            vec![String::from("H.264"), String::from("H.265")],
            vec![],
            Some("AV1"), // not in the list
            None,
            page,
            vec![],
        );
        // Falls back to index 0
        assert_eq!(state.settings.preset_index, 0);
        assert_eq!(state.settings.mode, "H.264");
    }
}
