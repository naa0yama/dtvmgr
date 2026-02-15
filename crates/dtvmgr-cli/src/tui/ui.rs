//! TUI rendering logic for the channel selector.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::state::{ActivePane, ChannelSelectorState, InputMode};

/// Draws the channel selector UI.
#[allow(clippy::indexing_slicing)]
pub fn draw(frame: &mut Frame, state: &ChannelSelectorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // main content
            Constraint::Length(3), // footer
        ])
        .split(frame.area());

    draw_header(frame, chunks[0], state);
    draw_main(frame, chunks[1], state);
    draw_footer(frame, chunks[2], state);
}

/// Draws the header with filter input and selection count.
#[allow(clippy::indexing_slicing)]
fn draw_header(frame: &mut Frame, area: Rect, state: &ChannelSelectorState) {
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let filter_style = if state.input_mode == InputMode::Filter {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let filter_text = if state.filter.is_empty() {
        String::new()
    } else {
        state.filter.clone()
    };

    let filter = Paragraph::new(filter_text)
        .style(filter_style)
        .block(Block::default().borders(Borders::ALL).title(" Filter: / "));
    frame.render_widget(filter, header_chunks[0]);

    let count_text = format!(
        "Selected: {} / {}",
        state.selected_count(),
        state.total_channels()
    );
    let count = Paragraph::new(count_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Channel Selector "),
    );
    frame.render_widget(count, header_chunks[1]);
}

/// Draws the main two-pane content.
#[allow(clippy::indexing_slicing)]
fn draw_main(frame: &mut Frame, area: Rect, state: &ChannelSelectorState) {
    let pane_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    draw_groups_pane(frame, pane_chunks[0], state);
    draw_channels_pane(frame, pane_chunks[1], state);
}

/// Draws the group list (left pane).
fn draw_groups_pane(frame: &mut Frame, area: Rect, state: &ChannelSelectorState) {
    let is_active = state.active_pane == ActivePane::Groups;
    let border_style = if is_active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let filtered = state.filtered_groups();
    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .filter_map(|(i, &group_idx)| {
            let group = state.groups.get(group_idx)?;
            let selected_count = state.selected_in_group(group_idx);
            let total_count = group.channels.len();

            let marker = if i == state.group_cursor && is_active {
                "\u{25b8} "
            } else {
                "  "
            };

            let style = if i == state.group_cursor && is_active {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if selected_count == total_count && total_count > 0 {
                Style::default().fg(Color::Green)
            } else if selected_count > 0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };

            Some(ListItem::new(Line::from(vec![
                Span::raw(String::from(marker)),
                Span::styled(
                    format!("{} {}/{}", group.name, selected_count, total_count),
                    style,
                ),
            ])))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Groups "),
    );

    frame.render_widget(list, area);
}

/// Draws the channel list (right pane).
fn draw_channels_pane(frame: &mut Frame, area: Rect, state: &ChannelSelectorState) {
    let is_active = state.active_pane == ActivePane::Channels;
    let border_style = if is_active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let group_idx = state.current_group_index();
    let empty: &[usize] = &[];
    let ch_indices = group_idx.map_or(empty, |idx| state.filtered_channels_for_group(idx));

    let items: Vec<ListItem> = ch_indices
        .iter()
        .enumerate()
        .filter_map(|(i, &ch_idx)| {
            let group = state.groups.get(group_idx?)?;
            let ch = group.channels.get(ch_idx)?;

            let checkbox = if state.selected.contains(&ch.ch_id) {
                "[x]"
            } else {
                "[ ]"
            };

            let style = if i == state.channel_cursor && is_active {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if state.selected.contains(&ch.ch_id) {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };

            Some(ListItem::new(Line::from(vec![Span::styled(
                format!(" {} {:>3}  {}", checkbox, ch.ch_id, ch.ch_name),
                style,
            )])))
        })
        .collect();

    let title = group_idx.map_or_else(
        || String::from(" Channels "),
        |idx| {
            state
                .groups
                .get(idx)
                .map_or_else(|| String::from(" Channels "), |g| format!(" {} ", g.name))
        },
    );

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title),
    );

    frame.render_widget(list, area);
}

/// Draws the footer with key hints.
fn draw_footer(frame: &mut Frame, area: Rect, state: &ChannelSelectorState) {
    let help_text = if state.input_mode == InputMode::Filter {
        "Type to filter | Esc: cancel filter | Enter: apply"
    } else {
        "Tab: pane switch  \u{2191}\u{2193}/j/k: move  Space: toggle  a: select all  A: deselect all  /: filter  Enter: confirm  q: cancel"
    };

    let footer = Paragraph::new(help_text).block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, area);
}
