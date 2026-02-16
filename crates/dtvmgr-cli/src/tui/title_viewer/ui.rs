//! TUI rendering logic for the title viewer.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};

use super::state::{ActivePane, InputMode, TitleViewerState};

/// Formats a number with thousands separators (e.g. 169940 -> "169,940").
#[allow(clippy::arithmetic_side_effects)]
fn fmt_num(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

/// Draws the title viewer UI. Returns the main content area height for page size calculation.
#[allow(clippy::indexing_slicing)]
pub fn draw(frame: &mut Frame, state: &mut TitleViewerState) -> u16 {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // header (2 lines of stats)
            Constraint::Min(5),    // main content
            Constraint::Length(3), // footer
        ])
        .split(frame.area());

    draw_header(frame, chunks[0], state);

    let main_area = chunks[1];
    let pane_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_area);

    draw_title_list(frame, pane_chunks[0], state);
    draw_program_detail(frame, pane_chunks[1], state);

    draw_footer(frame, chunks[2], state);

    main_area.height
}

/// Draws the header with filter input and title count.
#[allow(clippy::indexing_slicing)]
fn draw_header(frame: &mut Frame, area: Rect, state: &TitleViewerState) {
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

    let line1 = format!(
        "{} titles  {} progs  {}ch  (filtered: {})",
        fmt_num(state.stats.total_titles),
        fmt_num(state.stats.total_programs),
        fmt_num(state.stats.unique_channels),
        fmt_num(state.filtered_titles().len()),
    );
    let line2 = match (&state.stats.oldest_st_time, &state.stats.newest_st_time) {
        (Some(oldest), Some(newest)) => format!("{oldest} ~ {newest}"),
        _ => String::new(),
    };
    let count = Paragraph::new(vec![Line::from(line1), Line::from(line2)])
        .block(Block::default().borders(Borders::ALL).title(" DB Viewer "));
    frame.render_widget(count, header_chunks[1]);
}

/// Draws the title list pane (left).
fn draw_title_list(frame: &mut Frame, area: Rect, state: &mut TitleViewerState) {
    let border_style = if state.active_pane == ActivePane::Titles {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let header = Row::new(vec!["TID", "Title", "Year", "TMDB", "Season", "Progs"])
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1);

    let filtered = state.filtered_titles();
    let rows: Vec<Row> = filtered
        .iter()
        .filter_map(|&idx| {
            let t = state.titles.get(idx)?;

            let style = if t.tmdb_series_id.is_some() {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };

            let tmdb_str = t
                .tmdb_series_id
                .map_or_else(|| String::from("--"), |id| id.to_string());
            let season_str = t
                .tmdb_season_number
                .map_or_else(|| String::from("--"), |n| n.to_string());

            Some(
                Row::new(vec![
                    t.tid.to_string(),
                    t.title.clone(),
                    t.first_year
                        .map_or_else(|| String::from("--"), |y| y.to_string()),
                    tmdb_str,
                    season_str,
                    fmt_num(t.program_count),
                ])
                .style(style),
            )
        })
        .collect();

    let widths = [
        Constraint::Length(7),
        Constraint::Min(20),
        Constraint::Length(6),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(6),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Titles ")
                .border_style(border_style),
        )
        .row_highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_stateful_widget(table, area, &mut state.title_table_state);
}

/// Draws the program detail pane (right).
fn draw_program_detail(frame: &mut Frame, area: Rect, state: &mut TitleViewerState) {
    let border_style = if state.active_pane == ActivePane::Programs {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let title_info = state.current_title().map_or_else(
        || String::from(" Programs "),
        |t| format!(" {} (TID:{}) ", t.title, t.tid),
    );

    let header = Row::new(vec![
        "PID", "#", "StTime", "Min", "Channel", "Flag", "SubTitle",
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
    .bottom_margin(1);

    let programs = state.current_programs();
    let rows: Vec<Row> = programs
        .iter()
        .map(|p| {
            Row::new(vec![
                p.pid.to_string(),
                p.count.map_or_else(|| String::from("-"), |c| c.to_string()),
                p.st_time.clone(),
                p.duration_min
                    .map_or_else(|| String::from("-"), |m| m.to_string()),
                p.ch_name.clone(),
                flag_label(p.flag),
                p.sub_title.clone().unwrap_or_default(),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(10), // PID
        Constraint::Length(5),  // #
        Constraint::Length(20), // StTime
        Constraint::Length(5),  // Min
        Constraint::Length(15), // Channel
        Constraint::Length(8),  // Flag
        Constraint::Min(20),    // SubTitle
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title_info)
                .border_style(border_style),
        )
        .row_highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_stateful_widget(table, area, &mut state.program_table_state);
}

/// Builds a human-readable flag label from a bitmask.
fn flag_label(flag: Option<u32>) -> String {
    let Some(f) = flag else {
        return String::new();
    };
    let mut labels = Vec::new();
    if f & 1 != 0 {
        labels.push("[注]");
    }
    if f & 2 != 0 {
        labels.push("[新]");
    }
    if f & 4 != 0 {
        labels.push("[終]");
    }
    if f & 8 != 0 {
        labels.push("[再]");
    }
    labels.concat()
}

/// Draws the footer with key hints.
fn draw_footer(frame: &mut Frame, area: Rect, state: &TitleViewerState) {
    let help_text = match (&state.input_mode, &state.active_pane) {
        (InputMode::Filter, _) => Line::from("Type to filter | Esc: cancel | Enter: apply"),
        (InputMode::Normal, ActivePane::Titles) => Line::from(vec![Span::raw(
            "\u{2190}\u{2192}: pane  \u{2191}\u{2193}/j/k: move  PgUp/PgDn: page  /: filter  o: open  q: quit",
        )]),
        (InputMode::Normal, ActivePane::Programs) => Line::from(vec![Span::raw(
            "\u{2190}\u{2192}: pane  \u{2191}\u{2193}/j/k: move  PgUp/PgDn: page  o: open  q: quit",
        )]),
    };

    let footer = Paragraph::new(help_text).block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, area);
}
