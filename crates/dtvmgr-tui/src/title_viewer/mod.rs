//! Title/program viewer TUI main loop.

/// Title viewer state types.
pub mod state;
mod ui;

use std::collections::{HashMap, HashSet};
use std::io;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use regex::Regex;

use self::state::{ActivePane, InputMode, ProgramRow, TitleRow, TitleViewerState, ViewerStats};
use crate::normalize_viewer::state::normalize_chars;
use dtvmgr_db::channels::CachedChannel;
use dtvmgr_db::programs::CachedProgram;
use dtvmgr_db::titles::CachedTitle;

/// Extracts a base search query from a title using normalization and regex.
fn extract_base_query(title: &str, compiled_regex: Option<&Regex>) -> String {
    let normalized = normalize_chars(title);

    if let Some(re) = compiled_regex
        && let Some(m) = re.find(&normalized)
    {
        let mut result = String::with_capacity(normalized.len());
        result.push_str(&normalized[..m.start()]);
        result.push_str(&normalized[m.end()..]);
        let trimmed = result.trim().to_owned();
        if trimmed.is_empty() {
            normalized
        } else {
            trimmed
        }
    } else {
        normalized
    }
}

/// Result returned by the title viewer containing new TIDs to exclude.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct TitleViewerOutput {
    /// TIDs selected for exclusion during this session.
    pub new_excludes: Vec<u32>,
}

/// Builds a channel name lookup from cached channels.
fn build_channel_names(channels: Vec<CachedChannel>) -> HashMap<u32, String> {
    channels
        .into_iter()
        .map(|ch| (ch.ch_id, ch.ch_name))
        .collect()
}

/// Groups programs by TID with channel name resolution.
fn group_programs_by_tid(
    programs: &[CachedProgram],
    ch_names: &HashMap<u32, String>,
) -> HashMap<u32, Vec<ProgramRow>> {
    let mut programs_by_tid: HashMap<u32, Vec<ProgramRow>> = HashMap::new();
    for p in programs {
        let ch_name = ch_names
            .get(&p.ch_id)
            .cloned()
            .unwrap_or_else(|| p.ch_id.to_string());

        programs_by_tid.entry(p.tid).or_default().push(ProgramRow {
            pid: p.pid,
            count: p.count,
            st_time: p.st_time.clone(),
            ch_name,
            flag: p.flag,
            duration_min: p.duration_min,
            sub_title: p.st_sub_title.clone().or_else(|| p.sub_title.clone()),
        });
    }
    programs_by_tid
}

/// Computes viewer statistics from titles and programs.
fn compute_viewer_stats(titles: &[CachedTitle], programs: &[CachedProgram]) -> ViewerStats {
    let unique_channels = programs
        .iter()
        .map(|p| p.ch_id)
        .collect::<HashSet<_>>()
        .len();
    let oldest_st_time = programs
        .iter()
        .map(|p| p.st_time.as_str())
        .min()
        .map(String::from);
    let newest_st_time = programs
        .iter()
        .map(|p| p.st_time.as_str())
        .max()
        .map(String::from);

    let tmdb_matched = titles.iter().filter(|t| t.tmdb_series_id.is_some()).count();

    ViewerStats {
        total_titles: titles.len(),
        total_programs: programs.len(),
        unique_channels,
        oldest_st_time,
        newest_st_time,
        tmdb_matched,
    }
}

/// Builds title rows with TMDB query extraction.
fn build_title_rows(
    titles: &[CachedTitle],
    programs_by_tid: &HashMap<u32, Vec<ProgramRow>>,
    compiled_regex: Option<&Regex>,
) -> Vec<TitleRow> {
    titles
        .iter()
        .map(|t| {
            let tmdb_query = extract_base_query(&t.title, compiled_regex);
            TitleRow {
                tid: t.tid,
                title: t.title.clone(),
                cat: t.cat,
                first_year: t.first_year,
                tmdb_series_id: t.tmdb_series_id,
                tmdb_season_number: t.tmdb_season_number,
                program_count: programs_by_tid.get(&t.tid).map_or(0, Vec::len),
                keywords: dtvmgr_db::filter_keywords(
                    &t.keywords,
                    &t.title,
                    t.short_title.as_deref(),
                ),
                tmdb_query,
            }
        })
        .collect()
}

/// Launches the interactive title viewer TUI.
///
/// # Errors
///
/// Returns an error if terminal setup, event handling, or teardown fails.
#[allow(clippy::module_name_repetitions, clippy::implicit_hasher)]
pub fn run_title_viewer(
    titles: &[CachedTitle],
    programs: &[CachedProgram],
    channels: Vec<CachedChannel>,
    excluded_tids: HashSet<u32>,
    compiled_regex: Option<&regex::Regex>,
) -> Result<TitleViewerOutput> {
    let ch_names = build_channel_names(channels);
    let programs_by_tid = group_programs_by_tid(programs, &ch_names);
    let viewer_stats = compute_viewer_stats(titles, programs);
    let title_rows = build_title_rows(titles, &programs_by_tid, compiled_regex);

    let mut state = TitleViewerState::new(title_rows, programs_by_tid, viewer_stats, excluded_tids);

    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)
        .context("failed to enter alternate screen")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    let result = run_event_loop(&mut terminal, &mut state);

    // Cleanup (always attempt even if event loop failed)
    disable_raw_mode().context("failed to disable raw mode")?;
    crossterm::execute!(io::stdout(), LeaveAlternateScreen)
        .context("failed to leave alternate screen")?;

    result?;

    Ok(TitleViewerOutput {
        new_excludes: state.new_excludes(),
    })
}

/// Main event loop.
fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut TitleViewerState,
) -> Result<()> {
    let mut main_area_height: u16 = 0;

    loop {
        terminal
            .draw(|frame| {
                main_area_height = ui::draw(frame, state);
            })
            .context("failed to draw TUI")?;

        let page_size = usize::from(main_area_height.saturating_sub(4));

        if event::poll(std::time::Duration::from_millis(100)).context("failed to poll events")?
            && let Event::Key(key) = event::read().context("failed to read event")?
            && key.kind == KeyEventKind::Press
        {
            match state.input_mode {
                InputMode::Filter => {
                    if handle_filter_input(state, key.code) {
                        return Ok(());
                    }
                }
                InputMode::Normal => {
                    if handle_normal_input(state, key.code, key.modifiers, page_size) {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Handles key input in filter mode. Returns `true` to exit.
fn handle_filter_input(state: &mut TitleViewerState, key: KeyCode) -> bool {
    match key {
        KeyCode::Esc => {
            state.set_filter(String::new());
            state.input_mode = InputMode::Normal;
        }
        KeyCode::Enter => {
            state.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            state.filter_pop();
        }
        KeyCode::Char(c) => {
            state.filter_push(c);
        }
        _ => {}
    }
    false
}

/// Handles key input in normal mode. Returns `true` to exit.
fn handle_normal_input(
    state: &mut TitleViewerState,
    key: KeyCode,
    modifiers: KeyModifiers,
    page_size: usize,
) -> bool {
    match key {
        KeyCode::Char('q') => return true,
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Up | KeyCode::Char('k') => state.move_up(),
        KeyCode::Down | KeyCode::Char('j') => state.move_down(),
        KeyCode::Right if state.show_programs => state.focus_programs(),
        KeyCode::Left => state.focus_titles(),
        KeyCode::PageUp => state.page_up(page_size),
        KeyCode::PageDown => state.page_down(page_size),
        KeyCode::Char('/') => {
            if state.active_pane == ActivePane::Titles {
                state.input_mode = InputMode::Filter;
            }
        }
        KeyCode::Char('t') => state.toggle_tmdb_filter(),
        KeyCode::Char('p') => state.toggle_programs(),
        KeyCode::Char(' ') => state.toggle_select(),
        KeyCode::Char('o') => open_syoboi_url(state),
        _ => {}
    }
    false
}

/// Opens the Syoboi Calendar page for the current title or program.
#[allow(clippy::indexing_slicing)]
fn open_syoboi_url(state: &TitleViewerState) {
    let Some(title) = state.current_title() else {
        return;
    };
    let url = match state.active_pane {
        ActivePane::Titles => {
            format!("{}/tid/{}", dtvmgr_api::syoboi::SYOBOI_BASE_URL, title.tid)
        }
        ActivePane::Programs => {
            let programs = state.current_programs();
            let Some(prog) = programs.get(state.program_cursor()) else {
                return;
            };
            // st_time format: "YYYY-MM-DD HH:MM:SS" -> extract YYYYMM
            let yyyymm = prog.st_time.replace('-', "");
            let yyyymm = &yyyymm[..6];
            format!(
                "{}/tid/{}/summary/{yyyymm}",
                dtvmgr_api::syoboi::SYOBOI_BASE_URL,
                title.tid
            )
        }
    };
    let _ = open::that(&url);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use std::collections::{HashMap, HashSet};

    use crossterm::event::{KeyCode, KeyModifiers};
    use regex::Regex;

    use super::*;
    use crate::title_viewer::state::{
        ActivePane, InputMode, TitleRow, TitleViewerState, ViewerStats,
    };

    fn make_state() -> TitleViewerState {
        let titles = vec![
            TitleRow {
                tid: 1,
                title: String::from("SPY x FAMILY Season 2"),
                cat: Some(1),
                first_year: Some(2023),
                tmdb_series_id: Some(100),
                tmdb_season_number: Some(2),
                program_count: 1,
                keywords: Vec::new(),
                tmdb_query: String::from("SPY x FAMILY"),
            },
            TitleRow {
                tid: 2,
                title: String::from("Bocchi the Rock!"),
                cat: Some(1),
                first_year: Some(2022),
                tmdb_series_id: None,
                tmdb_season_number: None,
                program_count: 0,
                keywords: Vec::new(),
                tmdb_query: String::from("Bocchi the Rock!"),
            },
        ];
        let programs_by_tid = HashMap::new();
        let stats = ViewerStats {
            total_titles: 2,
            total_programs: 0,
            unique_channels: 0,
            oldest_st_time: None,
            newest_st_time: None,
            tmdb_matched: 1,
        };
        TitleViewerState::new(titles, programs_by_tid, stats, HashSet::new())
    }

    // ── extract_base_query ──────────────────────────────────────

    #[test]
    fn extract_base_query_without_regex_returns_normalized() {
        // Arrange & Act
        let result = extract_base_query("SPY×FAMILY Season 2", None);

        // Assert
        assert_eq!(result, "SPY×FAMILY Season 2");
    }

    #[test]
    fn extract_base_query_with_regex_removes_match() {
        // Arrange
        let re = Regex::new(r"Season\s+\d+").unwrap();

        // Act
        let result = extract_base_query("SPY×FAMILY Season 2", Some(&re));

        // Assert
        assert_eq!(result, "SPY×FAMILY");
    }

    #[test]
    fn extract_base_query_no_match_returns_normalized() {
        // Arrange
        let re = Regex::new(r"Season\s+\d+").unwrap();

        // Act
        let result = extract_base_query("Bocchi the Rock!", Some(&re));

        // Assert
        assert_eq!(result, "Bocchi the Rock!");
    }

    #[test]
    fn extract_base_query_full_match_returns_normalized() {
        // Arrange: regex matches entire string
        let re = Regex::new(r"^.*$").unwrap();

        // Act
        let result = extract_base_query("Test", Some(&re));

        // Assert: empty trimmed -> returns normalized
        assert_eq!(result, "Test");
    }

    // ── handle_filter_input ─────────────────────────────────────

    #[test]
    fn filter_input_esc_clears_filter_and_returns_normal() {
        // Arrange
        let mut state = make_state();
        state.input_mode = InputMode::Filter;
        state.filter_push('a');

        // Act
        let exit = handle_filter_input(&mut state, KeyCode::Esc);

        // Assert
        assert!(!exit);
        assert!(state.filter.is_empty());
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn filter_input_enter_returns_to_normal() {
        // Arrange
        let mut state = make_state();
        state.input_mode = InputMode::Filter;
        state.filter_push('x');

        // Act
        let exit = handle_filter_input(&mut state, KeyCode::Enter);

        // Assert
        assert!(!exit);
        assert_eq!(state.input_mode, InputMode::Normal);
        assert_eq!(state.filter, "x");
    }

    #[test]
    fn filter_input_backspace_pops() {
        // Arrange
        let mut state = make_state();
        state.filter_push('a');
        state.filter_push('b');

        // Act
        let exit = handle_filter_input(&mut state, KeyCode::Backspace);

        // Assert
        assert!(!exit);
        assert_eq!(state.filter, "a");
    }

    #[test]
    fn filter_input_char_pushes() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_filter_input(&mut state, KeyCode::Char('z'));

        // Assert
        assert!(!exit);
        assert_eq!(state.filter, "z");
    }

    #[test]
    fn filter_input_unknown_key_does_nothing() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_filter_input(&mut state, KeyCode::F(1));

        // Assert
        assert!(!exit);
        assert!(state.filter.is_empty());
    }

    // ── handle_normal_input ─────────────────────────────────────

    #[test]
    fn normal_input_q_exits() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('q'), KeyModifiers::NONE, 10);

        // Assert
        assert!(exit);
    }

    #[test]
    fn normal_input_ctrl_c_exits() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL, 10);

        // Assert
        assert!(exit);
    }

    #[test]
    fn normal_input_up_moves_up() {
        // Arrange
        let mut state = make_state();
        state.move_down();
        assert_eq!(state.title_cursor(), 1);

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Up, KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.title_cursor(), 0);
    }

    #[test]
    fn normal_input_down_moves_down() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Down, KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.title_cursor(), 1);
    }

    #[test]
    fn normal_input_k_moves_up() {
        // Arrange
        let mut state = make_state();
        state.move_down();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('k'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.title_cursor(), 0);
    }

    #[test]
    fn normal_input_j_moves_down() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('j'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.title_cursor(), 1);
    }

    #[test]
    fn normal_input_right_focuses_programs_when_visible() {
        // Arrange
        let mut state = make_state();
        assert!(state.show_programs);

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Right, KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.active_pane, ActivePane::Programs);
    }

    #[test]
    fn normal_input_left_focuses_titles() {
        // Arrange
        let mut state = make_state();
        state.focus_programs();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Left, KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.active_pane, ActivePane::Titles);
    }

    #[test]
    fn normal_input_page_up_down() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::PageDown, KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.title_cursor(), 1);

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::PageUp, KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.title_cursor(), 0);
    }

    #[test]
    fn normal_input_slash_enters_filter_mode_on_titles() {
        // Arrange
        let mut state = make_state();
        assert_eq!(state.active_pane, ActivePane::Titles);

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('/'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.input_mode, InputMode::Filter);
    }

    #[test]
    fn normal_input_slash_does_nothing_on_programs() {
        // Arrange
        let mut state = make_state();
        state.focus_programs();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('/'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert_eq!(state.input_mode, InputMode::Normal);
    }

    #[test]
    fn normal_input_t_toggles_tmdb_filter() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('t'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        // Cycled from All to Unmapped
        assert_eq!(state.filtered_titles().len(), 1);
    }

    #[test]
    fn normal_input_p_toggles_programs() {
        // Arrange
        let mut state = make_state();
        assert!(state.show_programs);

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char('p'), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert!(!state.show_programs);
    }

    #[test]
    fn normal_input_space_toggles_select() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::Char(' '), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
        assert!(state.selected_tids.contains(&1));
    }

    #[test]
    fn normal_input_unknown_key_does_nothing() {
        // Arrange
        let mut state = make_state();

        // Act
        let exit = handle_normal_input(&mut state, KeyCode::F(5), KeyModifiers::NONE, 10);

        // Assert
        assert!(!exit);
    }

    // ── build_channel_names ───────────────────────────────────────

    #[test]
    fn build_channel_names_creates_lookup() {
        // Arrange
        let channels = vec![
            CachedChannel {
                ch_id: 1,
                ch_gid: None,
                ch_name: String::from("NHK"),
            },
            CachedChannel {
                ch_id: 2,
                ch_gid: None,
                ch_name: String::from("TBS"),
            },
        ];

        // Act
        let names = build_channel_names(channels);

        // Assert
        assert_eq!(names.len(), 2);
        assert_eq!(names[&1], "NHK");
        assert_eq!(names[&2], "TBS");
    }

    #[test]
    fn build_channel_names_empty() {
        // Arrange & Act
        let names = build_channel_names(Vec::new());

        // Assert
        assert!(names.is_empty());
    }

    // ── group_programs_by_tid ─────────────────────────────────────

    #[test]
    fn group_programs_by_tid_groups_correctly() {
        // Arrange
        let ch_names = HashMap::from([(10, String::from("TOKYO MX"))]);
        let programs = vec![
            CachedProgram {
                pid: 100,
                tid: 1,
                ch_id: 10,
                tmdb_episode_id: None,
                st_time: String::from("2023-01-01 00:00:00"),
                st_offset: None,
                ed_time: String::from("2023-01-01 00:30:00"),
                count: Some(1),
                sub_title: Some(String::from("ep1")),
                flag: None,
                deleted: None,
                warn: None,
                revision: None,
                last_update: None,
                st_sub_title: None,
                duration_min: Some(30),
            },
            CachedProgram {
                pid: 101,
                tid: 1,
                ch_id: 10,
                tmdb_episode_id: None,
                st_time: String::from("2023-01-08 00:00:00"),
                st_offset: None,
                ed_time: String::from("2023-01-08 00:30:00"),
                count: Some(2),
                sub_title: None,
                flag: Some(1),
                deleted: None,
                warn: None,
                revision: None,
                last_update: None,
                st_sub_title: Some(String::from("st_ep2")),
                duration_min: Some(30),
            },
        ];

        // Act
        let grouped = group_programs_by_tid(&programs, &ch_names);

        // Assert
        assert_eq!(grouped.len(), 1);
        let rows = &grouped[&1];
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ch_name, "TOKYO MX");
        assert_eq!(rows[0].sub_title.as_deref(), Some("ep1"));
        // st_sub_title takes precedence over sub_title
        assert_eq!(rows[1].sub_title.as_deref(), Some("st_ep2"));
    }

    #[test]
    fn group_programs_by_tid_unknown_channel_uses_id() {
        // Arrange
        let ch_names = HashMap::new();
        let programs = vec![CachedProgram {
            pid: 200,
            tid: 5,
            ch_id: 99,
            tmdb_episode_id: None,
            st_time: String::from("2023-06-01 20:00:00"),
            st_offset: None,
            ed_time: String::from("2023-06-01 20:30:00"),
            count: None,
            sub_title: None,
            flag: None,
            deleted: None,
            warn: None,
            revision: None,
            last_update: None,
            st_sub_title: None,
            duration_min: None,
        }];

        // Act
        let grouped = group_programs_by_tid(&programs, &ch_names);

        // Assert
        assert_eq!(grouped[&5][0].ch_name, "99");
    }

    // ── compute_viewer_stats ──────────────────────────────────────

    #[test]
    fn compute_viewer_stats_with_data() {
        // Arrange
        let titles = vec![
            CachedTitle {
                tid: 1,
                tmdb_series_id: Some(100),
                tmdb_season_number: None,
                tmdb_season_id: None,
                title: String::from("Title A"),
                short_title: None,
                title_yomi: None,
                title_en: None,
                cat: None,
                title_flag: None,
                first_year: None,
                first_month: None,
                keywords: Vec::new(),
                sub_titles: None,
                last_update: String::new(),
                tmdb_original_name: None,
                tmdb_name: None,
                tmdb_alt_titles: None,
                tmdb_last_updated: None,
            },
            CachedTitle {
                tid: 2,
                tmdb_series_id: None,
                tmdb_season_number: None,
                tmdb_season_id: None,
                title: String::from("Title B"),
                short_title: None,
                title_yomi: None,
                title_en: None,
                cat: None,
                title_flag: None,
                first_year: None,
                first_month: None,
                keywords: Vec::new(),
                sub_titles: None,
                last_update: String::new(),
                tmdb_original_name: None,
                tmdb_name: None,
                tmdb_alt_titles: None,
                tmdb_last_updated: None,
            },
        ];
        let programs = vec![
            CachedProgram {
                pid: 10,
                tid: 1,
                ch_id: 1,
                tmdb_episode_id: None,
                st_time: String::from("2023-01-01 00:00:00"),
                st_offset: None,
                ed_time: String::from("2023-01-01 00:30:00"),
                count: None,
                sub_title: None,
                flag: None,
                deleted: None,
                warn: None,
                revision: None,
                last_update: None,
                st_sub_title: None,
                duration_min: None,
            },
            CachedProgram {
                pid: 11,
                tid: 2,
                ch_id: 2,
                tmdb_episode_id: None,
                st_time: String::from("2023-06-15 12:00:00"),
                st_offset: None,
                ed_time: String::from("2023-06-15 12:30:00"),
                count: None,
                sub_title: None,
                flag: None,
                deleted: None,
                warn: None,
                revision: None,
                last_update: None,
                st_sub_title: None,
                duration_min: None,
            },
        ];

        // Act
        let stats = compute_viewer_stats(&titles, &programs);

        // Assert
        assert_eq!(stats.total_titles, 2);
        assert_eq!(stats.total_programs, 2);
        assert_eq!(stats.unique_channels, 2);
        assert_eq!(stats.oldest_st_time.as_deref(), Some("2023-01-01 00:00:00"));
        assert_eq!(stats.newest_st_time.as_deref(), Some("2023-06-15 12:00:00"));
        assert_eq!(stats.tmdb_matched, 1);
    }

    #[test]
    fn compute_viewer_stats_empty() {
        // Arrange & Act
        let stats = compute_viewer_stats(&[], &[]);

        // Assert
        assert_eq!(stats.total_titles, 0);
        assert_eq!(stats.total_programs, 0);
        assert_eq!(stats.unique_channels, 0);
        assert!(stats.oldest_st_time.is_none());
        assert!(stats.newest_st_time.is_none());
        assert_eq!(stats.tmdb_matched, 0);
    }

    // ── build_title_rows ──────────────────────────────────────────

    #[test]
    fn build_title_rows_creates_rows() {
        // Arrange
        let titles = vec![CachedTitle {
            tid: 1,
            tmdb_series_id: Some(42),
            tmdb_season_number: Some(1),
            tmdb_season_id: None,
            title: String::from("SPY×FAMILY Season 2"),
            short_title: None,
            title_yomi: None,
            title_en: None,
            cat: Some(1),
            title_flag: None,
            first_year: Some(2023),
            first_month: None,
            keywords: Vec::new(),
            sub_titles: None,
            last_update: String::new(),
            tmdb_original_name: None,
            tmdb_name: None,
            tmdb_alt_titles: None,
            tmdb_last_updated: None,
        }];
        let programs_by_tid = HashMap::from([(
            1,
            vec![ProgramRow {
                pid: 100,
                count: Some(1),
                st_time: String::from("2023-10-07 23:00:00"),
                ch_name: String::from("TV Tokyo"),
                flag: None,
                duration_min: Some(30),
                sub_title: None,
            }],
        )]);
        let re = Regex::new(r"Season\s+\d+").unwrap();

        // Act
        let rows = build_title_rows(&titles, &programs_by_tid, Some(&re));

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tid, 1);
        assert_eq!(rows[0].program_count, 1);
        assert_eq!(rows[0].tmdb_query, "SPY×FAMILY");
        assert_eq!(rows[0].tmdb_series_id, Some(42));
        assert_eq!(rows[0].first_year, Some(2023));
    }

    #[test]
    fn build_title_rows_without_programs() {
        // Arrange
        let titles = vec![CachedTitle {
            tid: 5,
            tmdb_series_id: None,
            tmdb_season_number: None,
            tmdb_season_id: None,
            title: String::from("Bocchi the Rock!"),
            short_title: None,
            title_yomi: None,
            title_en: None,
            cat: Some(1),
            title_flag: None,
            first_year: Some(2022),
            first_month: None,
            keywords: Vec::new(),
            sub_titles: None,
            last_update: String::new(),
            tmdb_original_name: None,
            tmdb_name: None,
            tmdb_alt_titles: None,
            tmdb_last_updated: None,
        }];
        let programs_by_tid = HashMap::new();

        // Act
        let rows = build_title_rows(&titles, &programs_by_tid, None);

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].program_count, 0);
        assert_eq!(rows[0].tmdb_query, "Bocchi the Rock!");
    }
}
