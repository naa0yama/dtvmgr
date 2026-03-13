//! TUI rendering logic for the encode selector.

use std::fmt::Write;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use super::state::{EncodeSelectorState, InputMode, SettingsField, WizardStep};
use crate::tui::fmt::with_commas;

/// Formats bytes as MB with comma-separated thousands (e.g. "2,500 MB").
fn fmt_size(bytes: u64) -> String {
    let mb = bytes / 1_048_576;
    let mut s = with_commas(mb);
    s.push_str(" MB");
    s
}

/// Formats Unix ms timestamp as "YYYY-MM-DD HH:MM".
fn fmt_datetime(unix_ms: u64) -> String {
    #[allow(clippy::as_conversions, clippy::cast_possible_wrap)]
    let secs = (unix_ms / 1000) as i64;
    let dt = chrono::DateTime::from_timestamp(secs, 0);
    dt.map_or_else(
        || String::from("----"),
        |d| {
            use chrono::TimeZone;
            // JST offset (UTC+9) is always valid.
            #[allow(clippy::expect_used)]
            let local = chrono::FixedOffset::east_opt(9 * 3600)
                .expect("valid offset")
                .from_utc_datetime(&d.naive_utc());
            local.format("%Y-%m-%d %H:%M").to_string()
        },
    )
}

/// Formats duration in minutes from start/end timestamps.
fn fmt_duration(start_ms: u64, end_ms: u64) -> String {
    let mins = end_ms.saturating_sub(start_ms) / 60_000;
    format!("{mins}m")
}

/// Builds the encode queue display string for the header.
fn build_queue_display(state: &EncodeSelectorState) -> String {
    let Some(ref queue) = state.encode_queue else {
        return String::from(" Loading...");
    };

    let mut parts: Vec<String> = Vec::new();
    if queue.running.is_empty() {
        parts.push(String::from("Idle"));
    } else {
        for item in &queue.running {
            // Truncate name to 20 chars to fit in the header.
            let truncated: String = item.name.chars().take(20).collect();
            if let Some(pct) = item.percent {
                parts.push(format!("\u{25b6} {truncated} {} ({pct:.0}%)", item.mode));
            } else {
                parts.push(format!("\u{25b6} {truncated} {}", item.mode));
            }
        }
    }

    let mut s = format!(" {}", parts.join(" | "));
    let _ = write!(s, "  Que: {}", queue.waiting_count);
    s
}

/// Draws the encode selector UI.
#[allow(clippy::indexing_slicing)]
pub fn draw(frame: &mut Frame, state: &mut EncodeSelectorState) {
    match state.step {
        WizardStep::SelectRecordings => draw_recording_list(frame, state),
        WizardStep::ConfigureSettings => draw_settings(frame, state),
        WizardStep::Confirm => draw_confirm(frame, state),
    }
}

/// Step 1: Recording list with multi-select.
#[allow(clippy::indexing_slicing, clippy::too_many_lines)]
fn draw_recording_list(frame: &mut Frame, state: &mut EncodeSelectorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header / filter
            Constraint::Min(5),    // table
            Constraint::Length(3), // footer
        ])
        .split(frame.area());

    // Header: encode queue + selection count
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(chunks[0]);

    let queue_text = build_queue_display(state);
    let queue =
        Paragraph::new(queue_text).block(Block::default().borders(Borders::ALL).title(" Encode "));
    frame.render_widget(queue, header_chunks[0]);

    let mut count_parts = format!(
        " {} selected / {} shown / {} total",
        state.selected.len(),
        state.filtered_indices().len(),
        state.page.total,
    );
    if state.hide_unavailable {
        let hidden = state.hidden_count();
        let _ = write!(count_parts, " ({hidden} hidden)");
    }
    if let Some((fetched, total)) = state.sync_progress {
        let _ = write!(count_parts, " | Syncing: {fetched}/{total}");
    }
    if let Some(wp) = &state.file_check_progress {
        match wp.checking {
            Some((checked, total)) => {
                if wp.pending > 0 {
                    let _ = write!(
                        count_parts,
                        " | Checking: {checked}/{total} (+{} queued)",
                        wp.pending
                    );
                } else {
                    let _ = write!(count_parts, " | Checking: {checked}/{total}");
                }
            }
            None if wp.pending > 0 => {
                let _ = write!(count_parts, " | Queued: {}", wp.pending);
            }
            None => {}
        }
    }
    let _ = write!(
        count_parts,
        " | Page {}/{} ",
        state.current_page(),
        state.total_pages(),
    );
    let count = Paragraph::new(count_parts)
        .block(Block::default().borders(Borders::ALL).title(" Selection "));
    frame.render_widget(count, header_chunks[1]);

    // Table
    let header_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let header = Row::new(vec![
        Cell::from("Sel"),
        Cell::from("ID"),
        Cell::from("Channel"),
        Cell::from("Name"),
        Cell::from("Date"),
        Cell::from(Line::from("Dur").alignment(Alignment::Right)),
        Cell::from("Res"),
        Cell::from("Type"),
        Cell::from(Line::from("Size").alignment(Alignment::Right)),
        Cell::from(Line::from("Drop").alignment(Alignment::Right)),
        Cell::from(Line::from("Err").alignment(Alignment::Right)),
        Cell::from("Status"),
    ])
    .style(header_style);

    let rows: Vec<Row> = state
        .filtered_indices()
        .iter()
        .filter_map(|&idx| state.rows.get(idx))
        .map(|row| {
            let sel = if state.selected.contains(&row.recorded_id) {
                "[x]"
            } else {
                "[ ]"
            };
            let status = if row.is_recording {
                "rec"
            } else if row.is_encoding {
                "enc"
            } else {
                ""
            };

            let style = if !row.file_exists || row.source_video_file_id.is_none() {
                Style::default().fg(Color::Red)
            } else if state.selected.contains(&row.recorded_id) {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(String::from(sel)),
                Cell::from(row.recorded_id.to_string()),
                Cell::from(row.channel_name.clone()),
                Cell::from(row.name.clone()),
                Cell::from(fmt_datetime(row.start_at)),
                Cell::from(
                    Line::from(fmt_duration(row.start_at, row.end_at)).alignment(Alignment::Right),
                ),
                Cell::from(row.video_resolution.clone()),
                Cell::from(row.video_type.clone()),
                Cell::from(Line::from(fmt_size(row.file_size)).alignment(Alignment::Right)),
                Cell::from(Line::from(row.drop_cnt.to_string()).alignment(Alignment::Right)),
                Cell::from(Line::from(row.error_cnt.to_string()).alignment(Alignment::Right)),
                Cell::from(String::from(status)),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(3),
        Constraint::Length(7),
        Constraint::Length(9),
        Constraint::Min(20),
        Constraint::Length(17),
        Constraint::Length(5),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(10),
        Constraint::Length(5),
        Constraint::Length(5),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Recorded Programs "),
        )
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(Color::White),
        );

    frame.render_stateful_widget(table, chunks[1], &mut state.table_state);

    // Footer
    let mut footer_spans = vec![
        Span::styled(" Space", Style::default().fg(Color::Cyan)),
        Span::raw(":toggle "),
        Span::styled("a", Style::default().fg(Color::Cyan)),
        Span::raw(":all "),
        Span::styled("A", Style::default().fg(Color::Cyan)),
        Span::raw(":none "),
        Span::styled("f", Style::default().fg(Color::Cyan)),
        Span::raw(":avail "),
        Span::styled("R", Style::default().fg(Color::Cyan)),
        Span::raw(":refresh "),
    ];
    if state.has_prev_page() || state.has_next_page() {
        footer_spans.push(Span::styled("h/l", Style::default().fg(Color::Cyan)));
        footer_spans.push(Span::raw(":page "));
    }
    footer_spans.extend([
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(":next "),
        Span::styled("q/Esc", Style::default().fg(Color::Cyan)),
        Span::raw(":quit"),
    ]);
    let footer_text = Line::from(footer_spans);
    let footer =
        Paragraph::new(footer_text).block(Block::default().borders(Borders::ALL).title(" Keys "));
    frame.render_widget(footer, chunks[2]);
}

/// Step 2: Encode settings configuration.
#[allow(clippy::indexing_slicing, clippy::too_many_lines)]
fn draw_settings(frame: &mut Frame, state: &EncodeSelectorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(10),   // settings
            Constraint::Length(3), // footer
        ])
        .split(frame.area());

    // Header
    let header_text = format!(
        " Encode Settings — {} programs selected ",
        state.selected.len()
    );
    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL).title(" Step 2/3 "));
    frame.render_widget(header, chunks[0]);

    // Settings form
    let settings_area = chunks[1];
    let field_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // preset
            Constraint::Length(3), // save same dir
            Constraint::Length(3), // parent dir
            Constraint::Length(3), // directory
            Constraint::Length(3), // remove original
            Constraint::Min(1),    // spacer
        ])
        .split(settings_area);

    let highlight = |field: SettingsField| -> Style {
        if state.settings_field == field {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        }
    };

    // Preset
    let preset_text = format!(" Encode Preset: < {} >", state.settings.mode);
    let preset = Paragraph::new(preset_text)
        .style(highlight(SettingsField::Preset))
        .block(Block::default().borders(Borders::ALL).title(" Mode "));
    frame.render_widget(preset, field_chunks[0]);

    // Save same directory
    let same_dir_val = if state.settings.is_save_same_directory {
        "Yes"
    } else {
        "No"
    };
    let same_dir_text = format!(" Save in same directory: {same_dir_val}");
    let same_dir = Paragraph::new(same_dir_text)
        .style(highlight(SettingsField::SaveSameDirectory))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" isSaveSameDirectory "),
        );
    frame.render_widget(same_dir, field_chunks[1]);

    // Parent directory
    let parent_style = if state.settings.is_save_same_directory {
        Style::default().fg(Color::DarkGray)
    } else {
        highlight(SettingsField::ParentDir)
    };
    let parent_text = format!(" Parent Directory: < {} >", state.settings.parent_dir);
    let parent = Paragraph::new(parent_text)
        .style(parent_style)
        .block(Block::default().borders(Borders::ALL).title(" parentDir "));
    frame.render_widget(parent, field_chunks[2]);

    // Directory
    let dir_style = if state.input_mode == InputMode::DirectoryInput {
        Style::default().fg(Color::Yellow)
    } else {
        highlight(SettingsField::Directory)
    };
    let dir_text = format!(" Sub-directory: {}", state.settings.directory);
    let dir = Paragraph::new(dir_text).style(dir_style).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" directory (optional) "),
    );
    frame.render_widget(dir, field_chunks[3]);

    // Remove original
    let remove_val = if state.settings.remove_original {
        "Yes"
    } else {
        "No"
    };
    let remove_style = if state.settings.remove_original {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        highlight(SettingsField::RemoveOriginal)
    };
    let remove_text = format!(" Remove original file: {remove_val}");
    let remove = Paragraph::new(remove_text).style(remove_style).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" removeOriginal "),
    );
    frame.render_widget(remove, field_chunks[4]);

    // Footer
    let footer_text = Line::from(vec![
        Span::styled("j/k", Style::default().fg(Color::Cyan)),
        Span::raw(":move "),
        Span::styled("Space/←→", Style::default().fg(Color::Cyan)),
        Span::raw(":change "),
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(":next "),
        Span::styled("Esc", Style::default().fg(Color::Cyan)),
        Span::raw(":back"),
    ]);
    let footer =
        Paragraph::new(footer_text).block(Block::default().borders(Borders::ALL).title(" Keys "));
    frame.render_widget(footer, chunks[2]);
}

/// Step 3: Confirmation screen.
#[allow(clippy::indexing_slicing)]
fn draw_confirm(frame: &mut Frame, state: &EncodeSelectorState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(10),   // summary
            Constraint::Length(3), // footer
        ])
        .split(frame.area());

    // Header
    let header = Paragraph::new(" Confirm Encode ")
        .block(Block::default().borders(Borders::ALL).title(" Step 3/3 "));
    frame.render_widget(header, chunks[0]);

    // Summary
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(format!(
        "  Programs: {} selected",
        state.selected.len()
    )));
    lines.push(Line::from(format!("  Preset:   {}", state.settings.mode)));
    if state.settings.is_save_same_directory {
        lines.push(Line::from("  Output:   Same directory as source"));
    } else {
        lines.push(Line::from(format!(
            "  Output:   {} / {}",
            state.settings.parent_dir,
            if state.settings.directory.is_empty() {
                "(root)"
            } else {
                &state.settings.directory
            }
        )));
    }
    lines.push(Line::from(format!(
        "  Remove original: {}",
        if state.settings.remove_original {
            "Yes"
        } else {
            "No"
        }
    )));
    lines.push(Line::from(""));

    if state.settings.remove_original {
        lines.push(Line::from(Span::styled(
            format!(
                "  ⚠ WARNING: エンコード正常完了後、選択した {} 件のソースファイルが削除されます",
                state.selected.len()
            ),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        let remaining = state
            .required_confirms()
            .saturating_sub(state.confirm_count);
        lines.push(Line::from(format!(
            "  Press Enter {remaining} more time(s) to confirm"
        )));
    } else {
        lines.push(Line::from("  Press Enter to submit encode jobs"));
    }

    // Selected program names
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Selected programs:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    for row in &state.rows {
        if state.selected.contains(&row.recorded_id) {
            lines.push(Line::from(format!(
                "    [{}] {} — {}",
                row.recorded_id, row.channel_name, row.name
            )));
        }
    }

    let summary =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Summary "));
    frame.render_widget(summary, chunks[1]);

    // Footer
    let footer_text = Line::from(vec![
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(":submit "),
        Span::styled("Esc", Style::default().fg(Color::Cyan)),
        Span::raw(":back "),
        Span::styled("q", Style::default().fg(Color::Cyan)),
        Span::raw(":quit"),
    ]);
    let footer =
        Paragraph::new(footer_text).block(Block::default().borders(Borders::ALL).title(" Keys "));
    frame.render_widget(footer, chunks[2]);
}

/// Draws a loading screen with file check progress.
pub fn draw_loading_progress(frame: &mut Frame, page: u64, checked: usize, total: usize) {
    let area = frame.area();
    let text = if total == 0 {
        format!("Loading page {page} ...")
    } else {
        format!("Loading page {page} ... Checking files: {checked}/{total}")
    };
    let loading = Paragraph::new(text).alignment(Alignment::Center).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Recorded Programs "),
    );
    frame.render_widget(loading, area);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::tui::encode_selector::state::{EncodeQueueInfo, PageInfo, RunningEncodeItem};

    #[test]
    fn fmt_size_zero_bytes() {
        assert_eq!(fmt_size(0), "0 MB");
    }

    #[test]
    fn fmt_size_small_bytes() {
        assert_eq!(fmt_size(1000), "0 MB");
    }

    #[test]
    fn fmt_size_exact_megabyte() {
        assert_eq!(fmt_size(1_048_576), "1 MB");
    }

    #[test]
    fn fmt_size_500_mb() {
        assert_eq!(fmt_size(524_288_000), "500 MB");
    }

    #[test]
    fn fmt_size_2500_mb() {
        assert_eq!(fmt_size(2_621_440_000), "2,500 MB");
    }

    #[test]
    fn fmt_size_12500_mb() {
        assert_eq!(fmt_size(13_107_200_000), "12,500 MB");
    }

    #[test]
    fn build_queue_display_loading() {
        let state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 0,
                size: 10,
                total: 0,
            },
        );
        assert_eq!(build_queue_display(&state), " Loading...");
    }

    #[test]
    fn build_queue_display_idle() {
        let mut state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 0,
                size: 10,
                total: 0,
            },
        );
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![],
            waiting_count: 3,
        });
        let display = build_queue_display(&state);
        assert!(display.contains("Idle"));
        assert!(display.contains("Que: 3"));
    }

    #[test]
    fn build_queue_display_running() {
        let mut state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 0,
                size: 10,
                total: 0,
            },
        );
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![RunningEncodeItem {
                name: String::from("Test Program"),
                mode: String::from("H.264"),
                percent: Some(45.2),
            }],
            waiting_count: 1,
        });
        let display = build_queue_display(&state);
        assert!(display.contains("Test Program"));
        assert!(display.contains("H.264"));
        assert!(display.contains("45%"));
        assert!(display.contains("Que: 1"));
    }

    #[test]
    fn build_queue_display_multiple_running() {
        let mut state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 0,
                size: 10,
                total: 0,
            },
        );
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![
                RunningEncodeItem {
                    name: String::from("Program A"),
                    mode: String::from("H.264"),
                    percent: Some(50.0),
                },
                RunningEncodeItem {
                    name: String::from("Program B"),
                    mode: String::from("H.265"),
                    percent: None,
                },
            ],
            waiting_count: 2,
        });
        let display = build_queue_display(&state);
        // Two items joined by " | "
        assert!(display.contains("Program A"));
        assert!(display.contains("Program B"));
        assert!(display.contains(" | "));
        assert!(display.contains("50%"));
        assert!(display.contains("Que: 2"));
    }

    // ── fmt_datetime ─────────────────────────────────────────────

    #[test]
    fn fmt_datetime_zero() {
        // epoch(0) → 1970-01-01 09:00 in JST
        assert_eq!(fmt_datetime(0), "1970-01-01 09:00");
    }

    #[test]
    fn fmt_datetime_normal() {
        // 2024-01-15 20:00 JST = 2024-01-15 11:00 UTC = 1705316400 sec
        let unix_ms = 1_705_316_400_000;
        assert_eq!(fmt_datetime(unix_ms), "2024-01-15 20:00");
    }

    #[test]
    fn fmt_datetime_jst_date_boundary() {
        // 2024-01-15 23:30 UTC = 2024-01-16 08:30 JST
        let unix_ms = 1_705_361_400_000;
        assert_eq!(fmt_datetime(unix_ms), "2024-01-16 08:30");
    }

    // ── fmt_duration ─────────────────────────────────────────────

    #[test]
    fn fmt_duration_zero() {
        assert_eq!(fmt_duration(1000, 1000), "0m");
    }

    #[test]
    fn fmt_duration_normal() {
        // 30 minutes = 1_800_000 ms
        assert_eq!(fmt_duration(0, 1_800_000), "30m");
    }

    #[test]
    fn fmt_duration_start_after_end() {
        // saturating_sub returns 0
        assert_eq!(fmt_duration(2_000_000, 1_000_000), "0m");
    }

    // ── build_queue_display (additional) ─────────────────────────

    #[test]
    fn build_queue_display_truncates_long_name() {
        let mut state = EncodeSelectorState::new(
            vec![],
            vec![],
            vec![],
            None,
            None,
            PageInfo {
                offset: 0,
                size: 10,
                total: 0,
            },
        );
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![RunningEncodeItem {
                name: String::from("This Is A Very Long Program Name That Exceeds Twenty"),
                mode: String::from("H.265"),
                percent: None,
            }],
            waiting_count: 0,
        });
        let display = build_queue_display(&state);
        // Name should be truncated to 20 chars
        assert!(display.contains("This Is A Very Long "));
        // No percent shown
        assert!(!display.contains('%'));
        assert!(display.contains("H.265"));
    }
}
