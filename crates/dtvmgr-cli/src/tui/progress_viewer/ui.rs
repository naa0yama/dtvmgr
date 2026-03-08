//! TUI rendering logic for the progress viewer.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};

use super::state::ProgressViewerState;

/// Draws the progress viewer UI.
#[allow(clippy::indexing_slicing)]
pub fn draw(frame: &mut Frame, state: &ProgressViewerState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // stage info
            Constraint::Length(3), // progress bar
            Constraint::Min(5),    // log area
            Constraint::Length(3), // footer
        ])
        .split(frame.area());

    draw_stage(frame, chunks[0], state);
    draw_gauge(frame, chunks[1], state);
    draw_logs(frame, chunks[2], state);
    draw_footer(frame, chunks[3], state);
}

/// Draws the stage information.
fn draw_stage(frame: &mut Frame, area: ratatui::layout::Rect, state: &ProgressViewerState) {
    let text = if state.finished {
        Line::from(Span::styled(
            "Pipeline completed",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ))
    } else if state.current_stage == 0 {
        Line::from("Starting pipeline...")
    } else {
        let stage_text = format!(
            "({}/{}) {}",
            state.current_stage, state.total_stages, state.stage_status
        );
        Line::from(stage_text)
    };

    let paragraph = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Stage ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(paragraph, area);
}

/// Draws the unified progress gauge.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::as_conversions
)]
fn draw_gauge(frame: &mut Frame, area: ratatui::layout::Rect, state: &ProgressViewerState) {
    let pct = if state.finished {
        1.0
    } else {
        state.stage_percent
    };
    let percent_int = (pct * 100.0).round().clamp(0.0, 100.0) as u16;
    let label = format!("{percent_int}%");

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Progress ")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
        .percent(percent_int)
        .label(label);
    frame.render_widget(gauge, area);
}

/// Draws the scrolling log area.
#[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
fn draw_logs(frame: &mut Frame, area: ratatui::layout::Rect, state: &ProgressViewerState) {
    // Inner height excludes borders (top + bottom = 2).
    let inner_height = area.height.saturating_sub(2) as usize;

    let lines: Vec<Line<'_>> = if state.logs.is_empty() {
        vec![Line::from("Waiting for output...")]
    } else {
        let start = state.logs.len().saturating_sub(inner_height);
        state
            .logs
            .get(start..)
            .unwrap_or_default()
            .iter()
            .map(|s| Line::from(s.as_str()))
            .collect()
    };

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Log ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(paragraph, area);
}

/// Draws the footer with key hints.
fn draw_footer(frame: &mut Frame, area: ratatui::layout::Rect, state: &ProgressViewerState) {
    let help = if state.finished {
        "Press q to quit"
    } else {
        "Press q or Ctrl+C to cancel"
    };

    let footer = Paragraph::new(help).block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, area);
}
