//! TUI rendering logic for the encode selector.

use std::fmt::Write;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use super::state::{EncodeSelectorState, InputMode, SettingsField, WizardStep};
use crate::fmt::with_commas;

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

/// Builds two display lines for the encode header (always 2 lines).
///
/// Line 1: `▶ <title> <mode>  Que: N`
/// Line 2: `   (30%) <log>` or empty
fn build_queue_lines(state: &EncodeSelectorState) -> [Line<'static>; 2] {
    let Some(ref queue) = state.encode_queue else {
        return [Line::from(" Loading..."), Line::from("")];
    };

    if queue.running.is_empty() {
        let mut s = String::from(" Idle");
        let _ = write!(s, "  Que: {}", queue.waiting_count);
        return [Line::from(s), Line::from("")];
    }

    // Line 1: title + mode + queue count
    let mut titles: Vec<String> = Vec::new();
    for item in &queue.running {
        let truncated: String = item.name.chars().take(20).collect();
        titles.push(format!("\u{25b6} {truncated} {}", item.mode));
    }
    let mut line1 = format!(" {}", titles.join(" | "));
    let _ = write!(line1, "  Que: {}", queue.waiting_count);

    // Line 2: progress details (percent + log) for running items
    let mut progress_parts: Vec<String> = Vec::new();
    for item in &queue.running {
        let pct = item
            .percent
            .map(|p| format!("({:.0}%)", p * 100.0))
            .unwrap_or_default();
        let log = item.log.as_deref().unwrap_or_default();
        let part = format!("{pct} {log}").trim().to_owned();
        if !part.is_empty() {
            progress_parts.push(part);
        }
    }

    let line2 = if progress_parts.is_empty() {
        Line::from("")
    } else {
        Line::from(format!("   {}", progress_parts.join(" | ")))
    };

    [Line::from(line1), line2]
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
    let has_storage = !state.storage_dirs.is_empty();
    let visible_count = state.storage_dirs.iter().filter(|d| d.visible).count();

    // Storage widget: border(2) + visible dirs only. Hidden dirs are not rendered.
    let storage_height: u16 = if visible_count > 0 {
        u16::try_from(visible_count.saturating_add(2).min(12)).unwrap_or(12)
    } else {
        0
    };

    // Right column: Selection(3) + Storage. Encode fills the full left height.
    let selection_height: u16 = 3;
    let header_height = selection_height.saturating_add(storage_height).max(4);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height), // header row
            Constraint::Min(5),                // table
            Constraint::Length(3),             // footer
        ])
        .split(frame.area());

    // Header: left = Encode (full height), right = Selection + Storage
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    let queue_lines = build_queue_lines(state);
    let queue = Paragraph::new(queue_lines.to_vec())
        .block(Block::default().borders(Borders::ALL).title(" Encode "));
    frame.render_widget(queue, header_chunks[0]);

    if storage_height > 0 {
        // Right side: Selection on top, Storage below.
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(selection_height),
                Constraint::Length(storage_height),
            ])
            .split(header_chunks[1]);
        draw_selection_info(frame, right_chunks[0], state);
        draw_storage_stats(frame, right_chunks[1], state);
    } else {
        draw_selection_info(frame, header_chunks[1], state);
    }

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
        Cell::from("Files"),
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
            let queue_status = state.encode_queue_status(row.recorded_id);
            let status = if row.is_recording {
                "rec"
            } else if !queue_status.is_empty() {
                queue_status
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
                Cell::from(row.file_types.clone()),
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

    // Update visible row count: table area height minus borders(2) and header row(1).
    state.visible_table_rows = usize::from(chunks[1].height.saturating_sub(3));

    frame.render_stateful_widget(table, chunks[1], &mut state.table_state);

    // Footer
    let mut footer_spans = vec![
        Span::styled(" \u{2191}\u{2193}/jk", Style::default().fg(Color::Cyan)),
        Span::raw(":move "),
        Span::styled("Space", Style::default().fg(Color::Cyan)),
        Span::raw(":toggle "),
        Span::styled("a", Style::default().fg(Color::Cyan)),
        Span::raw(":all "),
        Span::styled("A", Style::default().fg(Color::Cyan)),
        Span::raw(":none "),
        Span::styled("f", Style::default().fg(Color::Cyan)),
        Span::raw(":avail "),
        Span::styled("e", Style::default().fg(Color::Cyan)),
        Span::raw(":\u{00ac}queue "),
        Span::styled("R", Style::default().fg(Color::Cyan)),
        Span::raw(":refresh "),
    ];
    footer_spans.push(Span::styled("PgUp/Dn", Style::default().fg(Color::Cyan)));
    footer_spans.push(Span::raw(":scroll "));
    if has_storage {
        footer_spans.push(Span::styled("1-9", Style::default().fg(Color::Cyan)));
        footer_spans.push(Span::raw(":storage "));
    }
    if state.has_prev_page() || state.has_next_page() {
        footer_spans.push(Span::styled(
            "\u{2190}\u{2192}/h/l",
            Style::default().fg(Color::Cyan),
        ));
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

/// Draws the selection count / sync / page info block.
fn draw_selection_info(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &EncodeSelectorState,
) {
    let mut text = format!(
        " {} sel / {} shown / {} total",
        state.selected.len(),
        state.filtered_indices().len(),
        state.page.total,
    );
    if state.hide_unavailable {
        let hidden = state.hidden_count();
        let _ = write!(text, " ({hidden} hid)");
    }
    if let Some((fetched, total)) = state.sync_progress {
        let _ = write!(text, " | Sync: {fetched}/{total}");
    }
    if let Some(wp) = &state.file_check_progress {
        match wp.checking {
            Some((checked, total)) => {
                if wp.pending > 0 {
                    let _ = write!(text, " | Chk: {checked}/{total} (+{})", wp.pending);
                } else {
                    let _ = write!(text, " | Chk: {checked}/{total}");
                }
            }
            None if wp.pending > 0 => {
                let _ = write!(text, " | Queued: {}", wp.pending);
            }
            None => {}
        }
    }
    let _ = write!(
        text,
        " | P.{}/{}",
        state.current_page(),
        state.total_pages()
    );
    let widget =
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" Selection "));
    frame.render_widget(widget, area);
}

/// Draws the storage stats widget.
#[allow(clippy::similar_names)]
fn draw_storage_stats(frame: &mut Frame, area: ratatui::layout::Rect, state: &EncodeSelectorState) {
    let dim = Style::default().fg(Color::DarkGray);

    // Calculate max name length for alignment.
    let max_name_len = state
        .storage_dirs
        .iter()
        .filter(|e| e.visible)
        .map(|e| e.name.len())
        .max()
        .unwrap_or(0);

    let lines: Vec<Line<'static>> = state
        .storage_dirs
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.visible)
        .map(|(i, entry)| {
            let key = char::from(u8::try_from(i).unwrap_or(8).saturating_add(b'1'));
            let prefix = format!(" {key}:{:<width$} ", entry.name, width = max_name_len);
            entry.stats.as_ref().map_or_else(
                || Line::from(vec![Span::raw(prefix.clone()), Span::styled("N/A", dim)]),
                |s| {
                    let used = fmt_gb(s.used_bytes);
                    let total = fmt_tb(s.total_bytes);
                    let pct = s.usage_ratio * 100.0;
                    let files = with_commas(s.file_count);
                    let color = if pct > 90.0 {
                        Color::Red
                    } else if pct > 75.0 {
                        Color::Yellow
                    } else {
                        Color::Green
                    };
                    Line::from(vec![
                        Span::raw(prefix.clone()),
                        Span::styled(format!("{used:>10}"), Style::default().fg(color)),
                        Span::raw(format!(" / {total:>6} ({pct:>5.1}%) {files:>7}")),
                        Span::styled(" files", dim),
                    ])
                },
            )
        })
        .collect();

    let widget =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Storage "));
    frame.render_widget(widget, area);
}

/// Formats bytes as a human-readable GB string (e.g. "1,234.5 GB").
fn fmt_gb(bytes: u64) -> String {
    let gb_10 = bytes / 107_374_182; // bytes / (1 GiB / 10)
    let whole = gb_10 / 10;
    let frac = gb_10 % 10;
    format!("{}.{frac} GB", with_commas(whole))
}

/// Formats bytes as a human-readable TB string (e.g. "3.6 TB").
fn fmt_tb(bytes: u64) -> String {
    let tb_10 = bytes / 109_951_162_778; // bytes / (1 TiB / 10)
    let whole = tb_10 / 10;
    let frac = tb_10 % 10;
    format!("{}.{frac} TB", with_commas(whole))
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
        Span::styled(" \u{2191}\u{2193}/jk", Style::default().fg(Color::Cyan)),
        Span::raw(":move "),
        Span::styled("Space/\u{2190}\u{2192}", Style::default().fg(Color::Cyan)),
        Span::raw(":change "),
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(":next "),
        Span::styled("Esc/q", Style::default().fg(Color::Cyan)),
        Span::raw(":back/quit"),
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
    #![allow(clippy::unwrap_used, clippy::as_conversions)]

    use std::collections::BTreeSet;

    use super::*;
    use crate::encode_selector::state::{EncodeQueueInfo, PageInfo, RunningEncodeItem};

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

    /// Helper: join all lines into a single string for assertion convenience.
    fn lines_to_string(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn build_queue_lines_loading() {
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
            vec![],
        );
        let lines = build_queue_lines(&state);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].to_string().contains("Loading..."));
    }

    #[test]
    fn build_queue_lines_idle() {
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
            vec![],
        );
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![],
            waiting_count: 3,
            waiting_ids: BTreeSet::new(),
        });
        let lines = build_queue_lines(&state);
        assert_eq!(lines.len(), 2);
        let display = lines_to_string(&lines);
        assert!(display.contains("Idle"));
        assert!(display.contains("Que: 3"));
    }

    #[test]
    fn build_queue_lines_running() {
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
            vec![],
        );
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![RunningEncodeItem {
                recorded_id: 1,
                name: String::from("Test Program"),
                mode: String::from("H.264"),
                percent: Some(0.452),
                log: Some(String::from("encoding 1500/3000")),
            }],
            waiting_count: 1,
            waiting_ids: BTreeSet::new(),
        });
        let queue_lines = build_queue_lines(&state);
        // Line 1: title + mode + queue count
        assert_eq!(queue_lines.len(), 2);
        let first = queue_lines[0].to_string();
        assert!(first.contains("Test Program"));
        assert!(first.contains("H.264"));
        assert!(first.contains("Que: 1"));
        // Line 2: progress details
        let second = queue_lines[1].to_string();
        assert!(second.contains("45%"));
        assert!(second.contains("encoding 1500/3000"));
    }

    #[test]
    fn build_queue_lines_multiple_running() {
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
            vec![],
        );
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![
                RunningEncodeItem {
                    recorded_id: 1,
                    name: String::from("Program A"),
                    mode: String::from("H.264"),
                    percent: Some(0.5),
                    log: None,
                },
                RunningEncodeItem {
                    recorded_id: 2,
                    name: String::from("Program B"),
                    mode: String::from("H.265"),
                    percent: None,
                    log: None,
                },
            ],
            waiting_count: 2,
            waiting_ids: BTreeSet::new(),
        });
        let lines = build_queue_lines(&state);
        let display = lines_to_string(&lines);
        // Line 1: titles
        assert!(display.contains("Program A"));
        assert!(display.contains("Program B"));
        assert!(display.contains(" | "));
        assert!(display.contains("Que: 2"));
        // Line 2: only Program A has percent
        assert!(display.contains("50%"));
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

    // ── Buffer rendering tests ─────────────────────────────────

    fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            s.push('\n');
        }
        s
    }

    fn make_draw_state(n: usize) -> EncodeSelectorState {
        use super::super::state::EncodeRow;
        let rows: Vec<EncodeRow> = (0..n)
            .map(|i| {
                #[allow(clippy::as_conversions)]
                let id = i as u64;
                EncodeRow {
                    recorded_id: id,
                    channel_name: String::from("NHK"),
                    name: format!("Program {i}"),
                    start_at: 1_705_316_400_000,
                    end_at: 1_705_318_200_000,
                    video_resolution: String::from("1080i"),
                    file_types: String::from("ts"),
                    source_video_file_id: Some(id),
                    file_size: 2_621_440_000,
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
            total: n as u64,
        };
        EncodeSelectorState::new(
            rows,
            vec![String::from("H.264"), String::from("H.265")],
            vec![String::from("recorded")],
            None,
            None,
            page,
            vec![],
        )
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn draw_select_recordings_step() {
        // Arrange
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_draw_state(3);
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![],
            waiting_count: 0,
            waiting_ids: BTreeSet::new(),
        });

        // Act
        terminal.draw(|frame| draw(frame, &mut state)).unwrap();

        // Assert
        let buf = terminal.backend().buffer();
        let content = buffer_to_string(buf);
        assert!(content.contains("Recorded Programs"));
        assert!(content.contains("Encode"));
        assert!(content.contains("Selection"));
        assert!(content.contains("Sel"));
        assert!(content.contains("Channel"));
        assert!(content.contains("Name"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn draw_configure_settings_step() {
        // Arrange
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_draw_state(2);
        state.step = WizardStep::ConfigureSettings;

        // Act
        terminal.draw(|frame| draw(frame, &mut state)).unwrap();

        // Assert
        let buf = terminal.backend().buffer();
        let content = buffer_to_string(buf);
        assert!(content.contains("Step 2/3"));
        assert!(content.contains("Encode Settings"));
        assert!(content.contains("Mode"));
        assert!(content.contains("H.264"));
        assert!(content.contains("isSaveSameDirectory"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn draw_confirm_step() {
        // Arrange
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_draw_state(2);
        state.selected.insert(0);
        state.step = WizardStep::Confirm;

        // Act
        terminal.draw(|frame| draw(frame, &mut state)).unwrap();

        // Assert
        let buf = terminal.backend().buffer();
        let content = buffer_to_string(buf);
        assert!(content.contains("Step 3/3"));
        assert!(content.contains("Confirm Encode"));
        assert!(content.contains("1 selected"));
        assert!(content.contains("Press Enter to submit"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn draw_loading_progress_zero_total() {
        // Arrange
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        // Act
        terminal
            .draw(|frame| draw_loading_progress(frame, 1, 0, 0))
            .unwrap();

        // Assert
        let buf = terminal.backend().buffer();
        let content = buffer_to_string(buf);
        assert!(content.contains("Loading page 1"));
        assert!(content.contains("Recorded Programs"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn draw_loading_progress_with_counts() {
        // Arrange
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        // Act
        terminal
            .draw(|frame| draw_loading_progress(frame, 2, 5, 10))
            .unwrap();

        // Assert
        let buf = terminal.backend().buffer();
        let content = buffer_to_string(buf);
        assert!(content.contains("Loading page 2"));
        assert!(content.contains("Checking files: 5/10"));
    }

    #[test]
    fn build_queue_lines_truncates_long_name() {
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
            vec![],
        );
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![RunningEncodeItem {
                recorded_id: 1,
                name: String::from("This Is A Very Long Program Name That Exceeds Twenty"),
                mode: String::from("H.265"),
                percent: None,
                log: None,
            }],
            waiting_count: 0,
            waiting_ids: BTreeSet::new(),
        });
        let lines = build_queue_lines(&state);
        assert_eq!(lines.len(), 2);
        let display = lines[0].to_string();
        // Name should be truncated to 20 chars
        assert!(display.contains("This Is A Very Long "));
        // No percent shown
        assert!(!display.contains('%'));
        assert!(display.contains("H.265"));
    }

    #[test]
    fn fmt_datetime_overflow_returns_fallback() {
        // Arrange: u64::MAX / 1000 wraps to negative i64, chrono returns None
        assert_eq!(fmt_datetime(u64::MAX), "----");
    }

    #[test]
    fn fmt_duration_partial_minute() {
        // 59 seconds = 59_000 ms → rounds down to 0m
        assert_eq!(fmt_duration(0, 59_000), "0m");
        // 90 seconds = 90_000 ms → 1m
        assert_eq!(fmt_duration(0, 90_000), "1m");
    }

    #[test]
    fn build_queue_lines_running_no_progress_empty_line2() {
        // Arrange: running item with no percent and no log → line2 empty
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
            vec![],
        );
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![RunningEncodeItem {
                recorded_id: 1,
                name: String::from("Test"),
                mode: String::from("H.264"),
                percent: None,
                log: None,
            }],
            waiting_count: 0,
            waiting_ids: BTreeSet::new(),
        });

        // Act
        let result = build_queue_lines(&state);

        // Assert
        let first = result[0].to_string();
        assert!(first.contains("Test"));
        assert!(first.contains("H.264"));
        // Line 2 should be empty since no progress info
        assert!(result[1].to_string().is_empty());
    }

    #[test]
    fn build_queue_lines_running_log_only_no_percent() {
        // Arrange: running item with log but no percent
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
            vec![],
        );
        state.encode_queue = Some(EncodeQueueInfo {
            running: vec![RunningEncodeItem {
                recorded_id: 1,
                name: String::from("Test"),
                mode: String::from("H.264"),
                percent: None,
                log: Some(String::from("muxing")),
            }],
            waiting_count: 0,
            waiting_ids: BTreeSet::new(),
        });

        // Act
        let result = build_queue_lines(&state);

        // Assert
        let second = result[1].to_string();
        assert!(second.contains("muxing"));
        // No percent shown
        assert!(!second.contains('%'));
    }

    #[test]
    fn fmt_size_large_value() {
        // 1 TB = 1_099_511_627_776 bytes = 1,048,576 MB
        assert_eq!(fmt_size(1_099_511_627_776), "1,048,576 MB");
    }
}
