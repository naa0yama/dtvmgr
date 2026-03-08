//! TUI rendering logic for the normalize viewer.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};

use super::state::{InputMode, NormalizeViewerState, RegexSource};

/// Draws the normalize viewer UI. Returns the main content area height.
#[allow(clippy::indexing_slicing)]
pub fn draw(frame: &mut Frame, state: &mut NormalizeViewerState) -> u16 {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // main content
            Constraint::Length(3), // footer
        ])
        .split(frame.area());

    draw_header(frame, chunks[0], state);

    let main_area = chunks[1];
    draw_table(frame, main_area, state);

    draw_footer(frame, chunks[2], state);

    main_area.height
}

/// Draws the header with regex/filter input and row counts.
#[allow(clippy::indexing_slicing, clippy::cast_possible_truncation)]
fn draw_header(frame: &mut Frame, area: Rect, state: &NormalizeViewerState) {
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let input_area = header_chunks[0];

    // Left pane: regex or filter input
    match state.input_mode {
        InputMode::Regex => {
            let border_color = if state.regex_error.is_some() {
                Color::Red
            } else {
                Color::Yellow
            };
            let input = Paragraph::new(state.regex_input.as_str())
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Regex: r ")
                        .border_style(Style::default().fg(border_color)),
                );
            frame.render_widget(input, input_area);

            // Show cursor at current position (offset by border)
            set_input_cursor(frame, input_area, state.regex_cursor_display_width());
        }
        InputMode::Filter => {
            let input = Paragraph::new(state.filter.as_str())
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Filter: / ")
                        .border_style(Style::default().fg(Color::Yellow)),
                );
            frame.render_widget(input, input_area);

            // Show cursor at end of filter text
            set_input_cursor(frame, input_area, state.filter.len());
        }
        InputMode::Normal => match state.regex_source {
            RegexSource::Manual => {
                let input = Paragraph::new(state.regex_input.as_str())
                    .style(Style::default().fg(Color::DarkGray))
                    .block(Block::default().borders(Borders::ALL).title(" Regex: r "));
                frame.render_widget(input, input_area);
            }
            RegexSource::Config => {
                let title = format!(" Regex: R [config: {}] ", state.regex_titles_count());
                let border_color = if state.regex_error.is_some() {
                    Color::Red
                } else {
                    Color::Green
                };
                let input = Paragraph::new("regex_titles")
                    .style(Style::default().fg(Color::Green))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(title)
                            .border_style(Style::default().fg(border_color)),
                    );
                frame.render_widget(input, input_area);
            }
        },
    }

    // Right pane: counts
    let selected_count = state.selected.len();
    let count_text = format!(
        "{} rows (f:{}) s:{}",
        state.rows.len(),
        state.filtered_indices().len(),
        selected_count,
    );
    let count = Paragraph::new(count_text)
        .block(Block::default().borders(Borders::ALL).title(" Normalize "));
    frame.render_widget(count, header_chunks[1]);
}

/// Draws the full-width title table.
fn draw_table(frame: &mut Frame, area: Rect, state: &mut NormalizeViewerState) {
    let header = Row::new(vec![
        "Sel",
        "TID",
        "Title",
        "BaseQuery",
        "Trim",
        "S",
        "Year",
        "Type",
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
    .bottom_margin(1);

    let filtered = state.filtered_indices().to_vec();
    let rows: Vec<Row> = filtered
        .iter()
        .filter_map(|&idx| {
            let r = state.rows.get(idx)?;

            let is_selected = state.selected.contains(&idx);
            let sel_mark = if is_selected { "[x]" } else { "[ ]" };

            let base_query = r.base_query.as_deref().unwrap_or(&r.normalized_title);
            let trim = r.trimmed.as_deref().unwrap_or("-");

            let season = r
                .season_num
                .map_or_else(|| String::from("-"), |s| s.to_string());

            let year = r
                .first_year
                .map_or_else(|| String::from("-"), |y| y.to_string());

            let media = format!("{}", r.media_type);

            let style = if is_selected {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };

            Some(
                Row::new(vec![
                    sel_mark.to_owned(),
                    r.tid.to_string(),
                    r.title.clone(),
                    base_query.to_owned(),
                    trim.to_owned(),
                    season,
                    year,
                    media,
                ])
                .style(style),
            )
        })
        .collect();

    let widths = [
        Constraint::Length(3), // Sel
        Constraint::Length(7), // TID
        Constraint::Min(20),   // Title
        Constraint::Min(18),   // BaseQuery
        Constraint::Min(10),   // Trim
        Constraint::Length(3), // S
        Constraint::Length(6), // Year
        Constraint::Length(5), // Type
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Titles ")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .row_highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_stateful_widget(table, area, &mut state.table_state);
}

/// Sets cursor position at the end of text inside a bordered input widget.
#[allow(clippy::cast_possible_truncation)]
fn set_input_cursor(frame: &mut Frame, area: Rect, text_len: usize) {
    let offset = u16::try_from(text_len).unwrap_or(u16::MAX);
    let cursor_x = area.x.saturating_add(1).saturating_add(offset);
    let cursor_y = area.y.saturating_add(1);
    frame.set_cursor_position(Position::new(cursor_x, cursor_y));
}

/// Draws the footer with key hints.
fn draw_footer(frame: &mut Frame, area: Rect, state: &NormalizeViewerState) {
    let help_text = match state.input_mode {
        InputMode::Filter => Line::from("Type to filter | Esc: cancel | Enter: apply"),
        InputMode::Regex => {
            let mut spans = vec![Span::raw(
                "Type regex | (?P<Name>...) = named group | Esc: cancel | Enter: apply",
            )];
            if let Some(ref err) = state.regex_error {
                spans.push(Span::styled(
                    format!("  Err: {err}"),
                    Style::default().fg(Color::Red),
                ));
            }
            Line::from(spans)
        }
        InputMode::Normal => {
            let source_hint = match state.regex_source {
                RegexSource::Manual => "R: config regex",
                RegexSource::Config => "R: manual regex",
            };
            Line::from(vec![Span::raw(format!(
                "r: edit regex  {source_hint}  Space: select  \u{2191}\u{2193}/j/k: move  Shift+\u{2191}\u{2193}/J/K: range  /: filter  q: quit",
            ))])
        }
    };

    let footer = Paragraph::new(help_text).block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, area);
}
