//! Channel selector state management.

use std::collections::{BTreeSet, HashMap};

/// Identifies which pane is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    /// Left pane: channel groups.
    Groups,
    /// Right pane: channels within the selected group.
    Channels,
}

/// A channel group with its channels.
#[derive(Debug, Clone)]
pub struct ChannelGroup {
    /// Channel group ID.
    pub ch_gid: u32,
    /// Group display name.
    pub name: String,
    /// Channels belonging to this group (sorted by `ch_id`).
    pub channels: Vec<ChannelEntry>,
}

/// A single channel entry.
#[derive(Debug, Clone)]
pub struct ChannelEntry {
    /// Channel ID.
    pub ch_id: u32,
    /// Channel name.
    pub ch_name: String,
}

/// Input mode for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Normal navigation mode.
    Normal,
    /// Filter text input mode.
    Filter,
}

/// Result of the TUI interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorResult {
    /// User confirmed the selection.
    Confirmed,
    /// User cancelled.
    Cancelled,
}

/// State for the channel selector TUI.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct ChannelSelectorState {
    /// All channel groups (ordered by display order).
    pub groups: Vec<ChannelGroup>,
    /// Currently selected channel IDs.
    pub selected: BTreeSet<u32>,
    /// Active pane.
    pub active_pane: ActivePane,
    /// Cursor position in the group list.
    pub group_cursor: usize,
    /// Cursor position in the channel list.
    pub channel_cursor: usize,
    /// Current input mode.
    pub input_mode: InputMode,
    /// Filter text.
    pub filter: String,
    /// Cached filtered group indices.
    filtered_group_indices: Vec<usize>,
    /// Cached filtered channel indices per group.
    filtered_channel_indices: HashMap<usize, Vec<usize>>,
}

impl ChannelSelectorState {
    /// Creates a new state from groups and initial selection.
    #[must_use]
    pub fn new(groups: Vec<ChannelGroup>, selected: BTreeSet<u32>) -> Self {
        let group_count = groups.len();
        let mut state = Self {
            groups,
            selected,
            active_pane: ActivePane::Groups,
            group_cursor: 0,
            channel_cursor: 0,
            input_mode: InputMode::Normal,
            filter: String::new(),
            filtered_group_indices: Vec::new(),
            filtered_channel_indices: HashMap::new(),
        };
        state.filtered_group_indices = (0..group_count).collect();
        state.rebuild_filter_cache();
        state
    }

    /// Returns the total number of channels across all groups.
    #[must_use]
    pub fn total_channels(&self) -> usize {
        self.groups.iter().map(|g| g.channels.len()).sum()
    }

    /// Returns the number of selected channels.
    #[must_use]
    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    /// Returns filtered group indices.
    #[must_use]
    pub fn filtered_groups(&self) -> &[usize] {
        &self.filtered_group_indices
    }

    /// Returns filtered channel indices for a group.
    #[must_use]
    pub fn filtered_channels_for_group(&self, group_idx: usize) -> &[usize] {
        self.filtered_channel_indices
            .get(&group_idx)
            .map_or(&[], Vec::as_slice)
    }

    /// Returns the actual group index for the current cursor position.
    #[must_use]
    pub fn current_group_index(&self) -> Option<usize> {
        self.filtered_group_indices.get(self.group_cursor).copied()
    }

    /// Returns the actual channel index for the current cursor position.
    #[must_use]
    pub fn current_channel_index(&self) -> Option<usize> {
        let group_idx = self.current_group_index()?;
        self.filtered_channel_indices
            .get(&group_idx)?
            .get(self.channel_cursor)
            .copied()
    }

    /// Count of selected channels in a given group.
    #[must_use]
    pub fn selected_in_group(&self, group_idx: usize) -> usize {
        self.groups.get(group_idx).map_or(0, |g| {
            g.channels
                .iter()
                .filter(|ch| self.selected.contains(&ch.ch_id))
                .count()
        })
    }

    /// Moves the cursor up in the active pane.
    #[allow(clippy::arithmetic_side_effects)]
    pub const fn move_up(&mut self) {
        match self.active_pane {
            ActivePane::Groups => {
                if self.group_cursor > 0 {
                    self.group_cursor -= 1;
                    self.channel_cursor = 0;
                }
            }
            ActivePane::Channels => {
                if self.channel_cursor > 0 {
                    self.channel_cursor -= 1;
                }
            }
        }
    }

    /// Moves the cursor down in the active pane.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn move_down(&mut self) {
        match self.active_pane {
            ActivePane::Groups => {
                if self.group_cursor + 1 < self.filtered_group_indices.len() {
                    self.group_cursor += 1;
                    self.channel_cursor = 0;
                }
            }
            ActivePane::Channels => {
                if let Some(group_idx) = self.current_group_index() {
                    let count = self
                        .filtered_channel_indices
                        .get(&group_idx)
                        .map_or(0, Vec::len);
                    if self.channel_cursor + 1 < count {
                        self.channel_cursor += 1;
                    }
                }
            }
        }
    }

    /// Switches active pane.
    pub const fn switch_pane(&mut self) {
        self.active_pane = match self.active_pane {
            ActivePane::Groups => ActivePane::Channels,
            ActivePane::Channels => ActivePane::Groups,
        };
    }

    /// Toggles selection for the current item.
    pub fn toggle_current(&mut self) {
        match self.active_pane {
            ActivePane::Groups => {
                if let Some(group_idx) = self.current_group_index() {
                    self.toggle_group(group_idx);
                }
            }
            ActivePane::Channels => {
                if let Some(group_idx) = self.current_group_index()
                    && let Some(ch_idx) = self.current_channel_index()
                    && let Some(ch) = self
                        .groups
                        .get(group_idx)
                        .and_then(|g| g.channels.get(ch_idx))
                {
                    let ch_id = ch.ch_id;
                    if self.selected.contains(&ch_id) {
                        self.selected.remove(&ch_id);
                    } else {
                        self.selected.insert(ch_id);
                    }
                }
            }
        }
    }

    /// Toggles all channels in a group.
    fn toggle_group(&mut self, group_idx: usize) {
        if let Some(group) = self.groups.get(group_idx) {
            let all_selected = group
                .channels
                .iter()
                .all(|ch| self.selected.contains(&ch.ch_id));

            if all_selected {
                for ch in &group.channels {
                    self.selected.remove(&ch.ch_id);
                }
            } else {
                for ch in &group.channels {
                    self.selected.insert(ch.ch_id);
                }
            }
        }
    }

    /// Selects all channels in the current group.
    pub fn select_all_in_group(&mut self) {
        if let Some(group_idx) = self.current_group_index()
            && let Some(group) = self.groups.get(group_idx)
        {
            for ch in &group.channels {
                self.selected.insert(ch.ch_id);
            }
        }
    }

    /// Deselects all channels in the current group.
    pub fn deselect_all_in_group(&mut self) {
        if let Some(group_idx) = self.current_group_index()
            && let Some(group) = self.groups.get(group_idx)
        {
            for ch in &group.channels {
                self.selected.remove(&ch.ch_id);
            }
        }
    }

    /// Updates the filter and rebuilds the cache.
    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.rebuild_filter_cache();
        self.group_cursor = 0;
        self.channel_cursor = 0;
    }

    /// Appends a character to the filter.
    pub fn filter_push(&mut self, ch: char) {
        self.filter.push(ch);
        self.rebuild_filter_cache();
        self.group_cursor = 0;
        self.channel_cursor = 0;
    }

    /// Removes the last character from the filter.
    pub fn filter_pop(&mut self) {
        self.filter.pop();
        self.rebuild_filter_cache();
    }

    /// Rebuilds the filter cache.
    fn rebuild_filter_cache(&mut self) {
        let filter_lower = self.filter.to_lowercase();
        self.filtered_group_indices.clear();
        self.filtered_channel_indices.clear();

        for (group_idx, group) in self.groups.iter().enumerate() {
            if self.filter.is_empty() {
                // No filter: show all
                self.filtered_group_indices.push(group_idx);
                let all_ch: Vec<usize> = (0..group.channels.len()).collect();
                self.filtered_channel_indices.insert(group_idx, all_ch);
            } else {
                // Filter: match group name or channel names
                let group_matches = group.name.to_lowercase().contains(&filter_lower);
                let matching_channels: Vec<usize> = group
                    .channels
                    .iter()
                    .enumerate()
                    .filter(|(_, ch)| {
                        group_matches || ch.ch_name.to_lowercase().contains(&filter_lower)
                    })
                    .map(|(i, _)| i)
                    .collect();

                if !matching_channels.is_empty() {
                    self.filtered_group_indices.push(group_idx);
                    self.filtered_channel_indices
                        .insert(group_idx, matching_channels);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::indexing_slicing)]

    use super::*;

    fn make_test_state() -> ChannelSelectorState {
        let groups = vec![
            ChannelGroup {
                ch_gid: 1,
                name: String::from("テレビ 関東"),
                channels: vec![
                    ChannelEntry {
                        ch_id: 3,
                        ch_name: String::from("フジテレビ"),
                    },
                    ChannelEntry {
                        ch_id: 4,
                        ch_name: String::from("日本テレビ"),
                    },
                ],
            },
            ChannelGroup {
                ch_gid: 2,
                name: String::from("BSデジタル"),
                channels: vec![ChannelEntry {
                    ch_id: 10,
                    ch_name: String::from("BS11"),
                }],
            },
        ];
        let selected = BTreeSet::from([3]);
        ChannelSelectorState::new(groups, selected)
    }

    #[test]
    fn test_initial_state() {
        // Arrange & Act
        let state = make_test_state();

        // Assert
        assert_eq!(state.total_channels(), 3);
        assert_eq!(state.selected_count(), 1);
        assert_eq!(state.active_pane, ActivePane::Groups);
        assert_eq!(state.group_cursor, 0);
        assert_eq!(state.channel_cursor, 0);
    }

    #[test]
    fn test_toggle_channel() {
        // Arrange
        let mut state = make_test_state();
        state.active_pane = ActivePane::Channels;

        // Act - toggle ch_id=3 (already selected)
        state.toggle_current();

        // Assert
        assert!(!state.selected.contains(&3));
    }

    #[test]
    fn test_toggle_group() {
        // Arrange
        let mut state = make_test_state();

        // Act - toggle group 0 (partially selected)
        state.toggle_current();

        // Assert - should select all in group
        assert!(state.selected.contains(&3));
        assert!(state.selected.contains(&4));
    }

    #[test]
    fn test_toggle_group_deselect() {
        // Arrange
        let mut state = make_test_state();
        state.selected.insert(4); // Now both in group 0 selected

        // Act
        state.toggle_current();

        // Assert - should deselect all
        assert!(!state.selected.contains(&3));
        assert!(!state.selected.contains(&4));
    }

    #[test]
    fn test_move_down_up() {
        // Arrange
        let mut state = make_test_state();

        // Act & Assert
        state.move_down();
        assert_eq!(state.group_cursor, 1);

        state.move_down(); // should stay at 1
        assert_eq!(state.group_cursor, 1);

        state.move_up();
        assert_eq!(state.group_cursor, 0);

        state.move_up(); // should stay at 0
        assert_eq!(state.group_cursor, 0);
    }

    #[test]
    fn test_switch_pane() {
        // Arrange
        let mut state = make_test_state();

        // Act
        state.switch_pane();
        assert_eq!(state.active_pane, ActivePane::Channels);

        state.switch_pane();
        assert_eq!(state.active_pane, ActivePane::Groups);
    }

    #[test]
    fn test_filter() {
        // Arrange
        let mut state = make_test_state();

        // Act
        state.set_filter(String::from("BS"));

        // Assert
        assert_eq!(state.filtered_groups().len(), 1);
        let group_idx = state.filtered_groups()[0];
        assert_eq!(state.groups[group_idx].name, "BSデジタル");
    }

    #[test]
    fn test_filter_by_channel_name() {
        // Arrange
        let mut state = make_test_state();

        // Act
        state.set_filter(String::from("フジ"));

        // Assert
        assert_eq!(state.filtered_groups().len(), 1);
        let group_idx = state.filtered_groups()[0];
        assert_eq!(state.groups[group_idx].name, "テレビ 関東");
        let ch_indices = state.filtered_channels_for_group(group_idx);
        assert_eq!(ch_indices.len(), 1);
    }

    #[test]
    fn test_selected_in_group() {
        // Arrange
        let state = make_test_state();

        // Assert
        assert_eq!(state.selected_in_group(0), 1); // ch_id=3 selected
        assert_eq!(state.selected_in_group(1), 0);
    }

    #[test]
    fn test_select_all_in_group() {
        // Arrange
        let mut state = make_test_state();

        // Act
        state.select_all_in_group();

        // Assert
        assert!(state.selected.contains(&3));
        assert!(state.selected.contains(&4));
    }

    #[test]
    fn test_deselect_all_in_group() {
        // Arrange
        let mut state = make_test_state();
        state.selected.insert(4);

        // Act
        state.deselect_all_in_group();

        // Assert
        assert!(!state.selected.contains(&3));
        assert!(!state.selected.contains(&4));
    }
}
