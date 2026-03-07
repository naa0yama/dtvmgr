//! dtvmgr - TV program data management CLI.

/// Application configuration (TOML).
mod config;
/// Terminal UI components.
mod tui;

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use tracing::instrument;
use tracing_subscriber::filter::EnvFilter;
#[cfg(not(feature = "otel"))]
use tracing_subscriber::fmt;
#[cfg(feature = "otel")]
use tracing_subscriber::layer::SubscriberExt;
#[cfg(feature = "otel")]
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::{AppConfig, load_or_fetch, resolve_config_path, resolve_data_dir};
use crate::tui::run_channel_selector;
use crate::tui::state::{ChannelEntry, ChannelGroup};
use dtvmgr_api::syoboi::{
    LocalSyoboiApi, ProgLookupParams, SyoboiClient, SyoboiProgram, SyoboiTitle,
    lookup_all_programs, resolve_time_range,
};
use dtvmgr_api::tmdb::{
    LocalTmdbApi, SearchMultiParams, TmdbClient, TmdbMediaType, TmdbMultiSearchResult,
};
use dtvmgr_db::channels::{CachedChannel, CachedChannelGroup};
use dtvmgr_db::programs::CachedProgram;
use dtvmgr_db::titles::CachedTitle;
use dtvmgr_db::{
    delete_programs_by_tids_not_in, delete_titles_by_cat_not_in, load_channels, load_programs,
    load_titles, load_titles_by_tids, open_db, update_tmdb_last_updated, update_tmdb_mapping,
    update_tmdb_search_result, upsert_channel_groups, upsert_channels, upsert_programs,
    upsert_titles,
};
use dtvmgr_jlse::channel::{detect_channel, load_channels as load_jlse_channels};
use dtvmgr_jlse::param::{detect_param, load_params};
use dtvmgr_jlse::pipeline::{PipelineContext, run_pipeline};
use dtvmgr_jlse::progress::ProgressMode;
use dtvmgr_jlse::settings::DataPaths;
use dtvmgr_jlse::types::{AvsTarget, JlseConfig};

/// CLI argument parser.
#[derive(Parser)]
#[command(about, version)]
struct Cli {
    /// Override config/data directory.
    #[arg(long, global = true)]
    dir: Option<PathBuf>,

    /// Subcommand to run.
    #[command(subcommand)]
    command: Commands,
}

/// Available subcommands.
#[derive(Subcommand)]
enum Commands {
    /// Query Syoboi Calendar API.
    Syoboi(SyoboiCommand),
    /// Query TMDB API.
    Tmdb(TmdbCommand),
    /// Local database operations.
    Db(DbCommand),
    /// CM detection pipeline (`join_logo_scp`).
    Jlse(JlseCommand),
}

/// Arguments for the `jlse` subcommand.
#[derive(clap::Args)]
struct JlseCommand {
    /// Jlse subcommand to run.
    #[command(subcommand)]
    command: JlseSubcommands,
}

/// Available jlse subcommands.
#[derive(Subcommand)]
enum JlseSubcommands {
    /// Detect broadcast channel from filename.
    Channel(JlseChannelArgs),
    /// Detect JL parameters from channel and filename.
    Param(JlseParamArgs),
    /// Run the full CM detection pipeline.
    Run(JlseRunArgs),
    /// Extract and display EIT program information via `TSDuck`.
    Tsduck(JlseTsduckArgs),
}

/// Encode target AVS selection for CLI.
#[derive(clap::ValueEnum, Clone, Copy, Default)]
enum AvsTargetArg {
    /// Cut CM only.
    Cutcm,
    /// Cut CM + logo removal.
    #[default]
    CutcmLogo,
}

impl From<AvsTargetArg> for AvsTarget {
    fn from(arg: AvsTargetArg) -> Self {
        match arg {
            AvsTargetArg::Cutcm => Self::CutCm,
            AvsTargetArg::CutcmLogo => Self::CutCmLogo,
        }
    }
}

/// Arguments for `jlse run`.
#[allow(clippy::struct_excessive_bools)]
#[derive(clap::Args)]
struct JlseRunArgs {
    /// Path to input .ts or .m2ts file.
    #[arg(short, long, required_unless_present = "epgstation")]
    input: Option<PathBuf>,
    /// Channel name override.
    #[arg(short, long)]
    channel: Option<String>,
    /// Encode target AVS.
    #[arg(short, long, value_enum, default_value_t)]
    target: AvsTargetArg,
    /// Enable `FFmpeg` filter output.
    #[arg(short, long)]
    filter: bool,
    /// Enable `FFmpeg` encoding.
    #[arg(short, long)]
    encode: bool,
    /// Additional `FFmpeg` options.
    #[arg(long)]
    ffmpeg_option: Option<String>,
    /// Encode output directory.
    #[arg(long)]
    outdir: Option<PathBuf>,
    /// Encode output filename.
    #[arg(long)]
    outname: Option<String>,
    /// Remove intermediate files after processing.
    #[arg(short, long)]
    remove: bool,
    /// Disable chapter addition (chapters are added by default).
    #[arg(long = "no-chapter", action = clap::ArgAction::SetFalse)]
    add_chapter: bool,
    /// Path to `tstables` binary (default: resolved from PATH).
    #[arg(long, default_value = "tstables")]
    tstables_bin: PathBuf,
    /// Enable EPGStation-compatible progress JSON output.
    /// Reads `INPUT` and `OUTPUT` from environment variables.
    #[arg(long)]
    epgstation: bool,
}

/// Arguments for `jlse channel`.
#[derive(clap::Args)]
struct JlseChannelArgs {
    /// Path to input .ts or .m2ts file.
    #[arg(short, long)]
    input: PathBuf,
    /// Channel name override (env: CHNNELNAME).
    #[arg(short, long)]
    channel: Option<String>,
}

/// Arguments for `jlse param`.
#[derive(clap::Args)]
struct JlseParamArgs {
    /// Path to input .ts or .m2ts file.
    #[arg(short, long)]
    input: PathBuf,
    /// Channel name override (env: CHNNELNAME).
    #[arg(short, long)]
    channel: Option<String>,
}

/// Arguments for `jlse tsduck`.
#[derive(clap::Args)]
struct JlseTsduckArgs {
    /// Path to input .ts or .m2ts file.
    #[arg(short, long)]
    input: PathBuf,
    /// Channel name override (for SID filtering via channel list).
    #[arg(short, long)]
    channel: Option<String>,
    /// Service ID to filter EIT events (decimal or hex `0x...`).
    #[arg(short, long)]
    sid: Option<String>,
    /// Path to `tstables` binary (default: resolved from PATH).
    #[arg(long, default_value = "tstables")]
    tstables_bin: PathBuf,
}

/// Arguments for the `db` subcommand.
#[derive(clap::Args)]
struct DbCommand {
    /// Db subcommand to run.
    #[command(subcommand)]
    command: DbSubcommands,
}

/// Available database subcommands.
#[derive(Subcommand)]
enum DbSubcommands {
    /// Sync Syoboi data to local database.
    Sync(DbSyncArgs),
    /// Browse cached titles and programs via TUI.
    List,
    /// Preview title normalization results via TUI.
    Normalize,
    /// Search TMDB for cached titles and store results.
    TmdbLookup(DbTmdbLookupArgs),
}

/// Arguments for the `db sync` subcommand.
#[derive(clap::Args)]
struct DbSyncArgs {
    /// Start datetime (default: now - 1 day).
    /// Formats: "2024-01-01T00:00:00", "2024-01-01 00:00:00", "2024-01-01".
    #[arg(long)]
    time_since: Option<String>,

    /// End datetime (default: now + 1 day). Same formats as --time-since.
    #[arg(long)]
    time_until: Option<String>,

    /// Comma-separated channel IDs. Falls back to config selected channels if omitted.
    #[arg(long, value_delimiter = ',')]
    ch_ids: Option<Vec<u32>>,
}

/// Arguments for the `db tmdb-lookup` subcommand.
#[derive(clap::Args)]
struct DbTmdbLookupArgs {
    /// Comma-separated title IDs. If omitted, searches all titles without TMDB mapping.
    #[arg(long, value_delimiter = ',')]
    tids: Option<Vec<u32>>,
    /// Response language (e.g. "ja-JP"). Falls back to config, then "en-US".
    #[arg(long)]
    language: Option<String>,
    /// Ignore cooldown and re-search all titles.
    #[arg(long)]
    force: bool,
    /// Retry only unmapped titles (`tmdb_series_id` is NULL), ignoring cooldown.
    #[arg(long)]
    retry_unmapped: bool,
}

/// Arguments for the `channels` subcommand.
#[derive(clap::Args)]
struct ChannelsCommand {
    /// Channels subcommand to run.
    #[command(subcommand)]
    command: ChannelsSubcommands,
}

/// Available channels subcommands.
#[derive(Subcommand)]
enum ChannelsSubcommands {
    /// Interactively select channels via TUI.
    Select,
    /// List currently selected channels.
    List,
}

/// Arguments for the `syoboi` subcommand.
#[derive(clap::Args)]
struct SyoboiCommand {
    /// Syoboi subcommand to run.
    #[command(subcommand)]
    command: SyoboiSubcommands,
}

/// Available Syoboi subcommands.
#[derive(Subcommand)]
enum SyoboiSubcommands {
    /// Query program schedule data (`ProgLookup`).
    Prog(ProgArgs),
    /// Query title data (`TitleLookup`).
    Titles(TitlesArgs),
    /// Manage channel selection.
    Channels(ChannelsCommand),
}

/// Arguments for the `syoboi prog` subcommand.
#[derive(clap::Args)]
struct ProgArgs {
    /// Start datetime (default: now - 1 day).
    /// Formats: "2024-01-01T00:00:00", "2024-01-01 00:00:00", "2024-01-01".
    #[arg(long)]
    time_since: Option<String>,

    /// End datetime (default: now + 1 day). Same formats as --time-since.
    #[arg(long)]
    time_until: Option<String>,

    /// Comma-separated channel IDs (e.g. "1,7,19"). Falls back to config selected channels if omitted.
    #[arg(long, value_delimiter = ',')]
    ch_ids: Option<Vec<u32>>,
}

/// Arguments for the `syoboi titles` subcommand.
#[derive(clap::Args)]
struct TitlesArgs {
    /// Comma-separated title IDs (e.g. "6309,7667").
    #[arg(long, required = true, value_delimiter = ',')]
    tids: Vec<u32>,
}

/// Arguments for the `tmdb` subcommand.
#[derive(clap::Args)]
struct TmdbCommand {
    /// TMDB subcommand to run.
    #[command(subcommand)]
    command: TmdbSubcommands,
}

/// Available TMDB subcommands.
#[derive(Subcommand)]
enum TmdbSubcommands {
    /// Search for TV series on TMDB.
    SearchTv(TmdbSearchTvArgs),
    /// Search for movies on TMDB.
    SearchMovie(TmdbSearchMovieArgs),
    /// Get TV series details from TMDB.
    TvDetails(TmdbTvDetailsArgs),
    /// Get TV season details from TMDB.
    TvSeason(TmdbTvSeasonArgs),
}

/// Arguments for the `tmdb search-tv` subcommand.
#[derive(clap::Args)]
struct TmdbSearchTvArgs {
    /// Search query (e.g. "SPY×FAMILY").
    #[arg(long, required = true)]
    query: String,
    /// Response language (e.g. "ja-JP"). Falls back to config, then "en-US".
    #[arg(long)]
    language: Option<String>,
}

/// Arguments for the `tmdb search-movie` subcommand.
#[derive(clap::Args)]
struct TmdbSearchMovieArgs {
    /// Search query (e.g. "すずめの戸締まり").
    #[arg(long, required = true)]
    query: String,
    /// Response language (e.g. "ja-JP"). Falls back to config, then "en-US".
    #[arg(long)]
    language: Option<String>,
}

/// Arguments for the `tmdb tv-details` subcommand.
#[derive(clap::Args)]
struct TmdbTvDetailsArgs {
    /// TMDB series ID.
    #[arg(long, required = true)]
    id: u64,
    /// Response language (e.g. "ja-JP"). Falls back to config, then "en-US".
    #[arg(long)]
    language: Option<String>,
}

/// Arguments for the `tmdb tv-season` subcommand.
#[derive(clap::Args)]
struct TmdbTvSeasonArgs {
    /// TMDB series ID.
    #[arg(long, required = true)]
    id: u64,
    /// Season number.
    #[arg(long, required = true)]
    season: u32,
    /// Response language (e.g. "ja-JP"). Falls back to config, then "en-US".
    #[arg(long)]
    language: Option<String>,
}

/// Runs the `syoboi prog` subcommand.
///
/// Falls back to config selected channels when `--ch-ids` is not specified.
///
/// # Errors
///
/// Returns an error if the API client fails to build, time range is invalid,
/// or the API request fails.
#[instrument(skip_all)]
async fn run_syoboi_prog(args: &ProgArgs, dir: Option<&PathBuf>) -> Result<()> {
    let client = SyoboiClient::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("failed to build API client")?;

    let range = resolve_time_range(args.time_since.as_deref(), args.time_until.as_deref())?;
    tracing::info!(
        "Time range: {} .. {}",
        range.start.format("%Y-%m-%d %H:%M:%S"),
        range.end.format("%Y-%m-%d %H:%M:%S"),
    );

    let ch_ids = resolve_ch_ids(args.ch_ids.clone(), dir)?;

    let params = ProgLookupParams {
        ch_ids: Some(ch_ids),
        range: Some(range),
        ..ProgLookupParams::default()
    };

    let programs = lookup_all_programs(&client, &params)
        .await
        .context("failed to fetch programs")?;

    tracing::info!("PID\t\tTID\tChID\tCount\tStTime\t\t\tEdTime\t\t\tSubTitle");
    for prog in &programs {
        tracing::info!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            prog.pid,
            prog.tid,
            prog.ch_id,
            prog.count
                .map_or_else(|| String::from("-"), |c| c.to_string()),
            prog.st_time,
            prog.ed_time,
            prog.st_sub_title.as_deref().unwrap_or("-"),
        );
    }
    tracing::info!("Total: {} programs", programs.len());

    Ok(())
}

/// Runs the `syoboi titles` subcommand.
///
/// # Errors
///
/// Returns an error if the API client fails to build or the API request fails.
#[instrument(skip_all)]
async fn run_syoboi_titles(args: &TitlesArgs) -> Result<()> {
    let client = SyoboiClient::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("failed to build API client")?;

    let titles = client
        .lookup_titles(&args.tids, None)
        .await
        .context("failed to fetch titles")?;

    tracing::info!("TID\tTitle\t\t\tFirstYear\tFirstMonth\tFirstCh\t\tUserPoint");
    for title in &titles {
        tracing::info!(
            "{}\t{}\t{}\t\t{}\t\t{}\t\t{}",
            title.tid,
            title.title,
            title
                .first_year
                .map_or_else(|| String::from("-"), |v| v.to_string()),
            title
                .first_month
                .map_or_else(|| String::from("-"), |v| v.to_string()),
            title.first_ch.as_deref().unwrap_or("-"),
            title
                .user_point
                .map_or_else(|| String::from("-"), |v| v.to_string()),
        );
    }
    tracing::info!("Total: {} titles", titles.len());

    Ok(())
}

/// Resolves channel IDs from CLI args or config fallback.
///
/// Returns an error if no channels are specified via `--ch-ids` or config.
fn resolve_ch_ids(ch_ids: Option<Vec<u32>>, dir: Option<&PathBuf>) -> Result<Vec<u32>> {
    if let Some(ids) = ch_ids {
        return Ok(ids);
    }

    let config_path = resolve_config_path(dir).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;
    if config.syoboi.channels.selected.is_empty() {
        anyhow::bail!(
            "No channels selected. Run `dtvmgr syoboi channels select` first, \
             or pass --ch-ids explicitly."
        );
    }
    tracing::info!(
        "Using {} channel(s) from config: {:?}",
        config.syoboi.channels.selected.len(),
        config.syoboi.channels.selected
    );
    Ok(config.syoboi.channels.selected)
}

/// Title lookup chunk size for Syoboi API.
const TITLE_LOOKUP_CHUNK_SIZE: usize = 50;

/// Max retries per title chunk (rate-limit empty-response recovery).
///
/// Cloudflare rate-limit on cal.syoboi.jp typically lasts ~30-35s.
/// With initial backoff 10s: 10+20+40+80+160 = 310s theoretical max,
/// but usually resolves by retry 2-3 (cumulative 30-70s).
const TITLE_CHUNK_MAX_RETRIES: u32 = 5;

/// Initial backoff before retrying a title chunk (doubles each retry).
const TITLE_CHUNK_INITIAL_BACKOFF: Duration = Duration::from_secs(10);

/// Fields to request from `TitleLookup` during db sync.
///
/// Excludes `Comment` (contains unescaped `&` in URLs that breaks XML parsing)
/// and other fields unused by `to_cached_title`.
const TITLE_SYNC_FIELDS: &[&str] = &[
    "TID",
    "LastUpdate",
    "Title",
    "ShortTitle",
    "TitleYomi",
    "TitleEN",
    "Cat",
    "TitleFlag",
    "FirstYear",
    "FirstMonth",
    "Keywords",
    "SubTitles",
];

/// Runs the `db sync` subcommand.
///
/// Fetches programs and titles from Syoboi API and upserts into local DB.
///
/// # Errors
///
/// Returns an error if API calls or DB operations fail.
#[instrument(skip_all)]
/// Converts a `SyoboiTitle` to a `CachedTitle` for DB storage.
fn to_cached_title(t: &SyoboiTitle) -> CachedTitle {
    CachedTitle {
        tid: t.tid,
        tmdb_series_id: None,
        tmdb_season_number: None,
        tmdb_season_id: None,
        title: t.title.clone(),
        short_title: t.short_title.clone(),
        title_yomi: t.title_yomi.clone(),
        title_en: t.title_en.clone(),
        cat: t.cat,
        title_flag: t.title_flag,
        first_year: t.first_year,
        first_month: t.first_month,
        keywords: dtvmgr_db::parse_keywords(t.keywords.clone()),
        sub_titles: t.sub_titles.clone(),
        last_update: t.last_update.clone(),
        tmdb_original_name: None,
        tmdb_name: None,
        tmdb_alt_titles: None,
        tmdb_last_updated: None,
    }
}

/// Converts a `SyoboiProgram` to a `CachedProgram` for DB storage.
fn to_cached_program(p: &SyoboiProgram) -> CachedProgram {
    CachedProgram {
        pid: p.pid,
        tid: p.tid,
        ch_id: p.ch_id,
        tmdb_episode_id: None,
        st_time: p.st_time.clone(),
        st_offset: p.st_offset,
        ed_time: p.ed_time.clone(),
        count: p.count,
        sub_title: p.sub_title.clone(),
        flag: p.flag,
        deleted: p.deleted,
        warn: p.warn,
        revision: p.revision,
        last_update: p.last_update.clone(),
        st_sub_title: p.st_sub_title.clone(),
        duration_min: None,
    }
}

/// Fetches titles in chunks with retry + exponential backoff for empty responses.
///
/// When the API returns an empty response for a non-empty chunk (likely
/// rate-limited), retries up to `TITLE_CHUNK_MAX_RETRIES` times with
/// exponential backoff starting at `TITLE_CHUNK_INITIAL_BACKOFF`.
#[allow(clippy::arithmetic_side_effects)]
async fn fetch_titles_chunked(
    client: &SyoboiClient,
    unique_tids: &[u32],
) -> Result<Vec<SyoboiTitle>> {
    let mut all_titles = Vec::new();
    let chunks: Vec<&[u32]> = unique_tids.chunks(TITLE_LOOKUP_CHUNK_SIZE).collect();
    let total_chunks = chunks.len();

    for (i, chunk) in chunks.into_iter().enumerate() {
        tracing::debug!(?chunk, "TitleLookup requesting TIDs");

        let mut titles = Vec::new();
        let mut last_code: u16 = 0;
        for retry in 0..=TITLE_CHUNK_MAX_RETRIES {
            let (code, result) = client
                .lookup_titles_with_status(chunk, Some(TITLE_SYNC_FIELDS))
                .await
                .with_context(|| {
                    format!("failed to fetch titles for chunk of {} TIDs", chunk.len())
                })?;
            last_code = code;

            if !result.is_empty() || chunk.is_empty() {
                titles = result;
                break;
            }

            // Empty response for a non-empty chunk — likely rate-limited.
            if retry < TITLE_CHUNK_MAX_RETRIES {
                let backoff = TITLE_CHUNK_INITIAL_BACKOFF * 2u32.pow(retry);
                tracing::warn!(
                    chunk = i + 1,
                    total_chunks,
                    code,
                    retry = retry + 1,
                    max_retries = TITLE_CHUNK_MAX_RETRIES,
                    backoff_secs = backoff.as_secs(),
                    "TitleLookup returned 0 titles for non-empty chunk, retrying after backoff"
                );
                tokio::time::sleep(backoff).await;
            } else {
                tracing::warn!(
                    chunk = i + 1,
                    total_chunks,
                    code,
                    requested = chunk.len(),
                    "TitleLookup returned 0 titles after all retries, skipping chunk"
                );
            }
        }

        if titles.is_empty() {
            tracing::warn!(
                chunk = i + 1,
                total_chunks,
                code = last_code,
                fetched = 0,
                "TitleLookup chunk completed"
            );
        } else {
            tracing::info!(
                chunk = i + 1,
                total_chunks,
                code = last_code,
                fetched = titles.len(),
                "TitleLookup chunk completed"
            );
        }
        all_titles.extend(titles);
    }

    Ok(all_titles)
}

/// Filters and upserts programs, skipping those with missing FK references.
///
/// `all_fetched_tids` contains TIDs from all API-fetched titles (before cat
/// filtering) and is used to distinguish cat-filtered skips from genuine
/// FK misses.
fn upsert_filtered_programs(
    conn: &dtvmgr_db::Connection,
    programs: &[SyoboiProgram],
    valid_tids: &HashSet<u32>,
    valid_ch_ids: &HashSet<u32>,
    all_fetched_tids: &HashSet<u32>,
) -> Result<(usize, usize)> {
    let mut cat_filtered: usize = 0;
    let mut fk_missing: usize = 0;
    let cached: Vec<CachedProgram> = programs
        .iter()
        .filter(|p| {
            if valid_tids.contains(&p.tid) && valid_ch_ids.contains(&p.ch_id) {
                return true;
            }
            if all_fetched_tids.contains(&p.tid) && !valid_tids.contains(&p.tid) {
                cat_filtered = cat_filtered.saturating_add(1);
            } else {
                fk_missing = fk_missing.saturating_add(1);
            }
            false
        })
        .map(to_cached_program)
        .collect();
    if cat_filtered > 0 {
        tracing::info!(
            skipped = cat_filtered,
            "Skipped programs (title excluded by cat filter)"
        );
    }
    if fk_missing > 0 {
        tracing::warn!(
            skipped = fk_missing,
            "Skipped programs with missing FK references"
        );
    }
    let changed = upsert_programs(conn, &cached).context("failed to upsert programs")?;
    tracing::info!(
        changed,
        unchanged = cached.len().saturating_sub(changed),
        "Programs upsert complete"
    );
    Ok((cached.len(), changed))
}

/// Deletes titles and programs whose categories are not in the allowed set.
fn cleanup_disallowed_cats(
    conn: &dtvmgr_db::Connection,
    allowed_cats: &HashSet<u32>,
) -> Result<()> {
    let allowed_cats_vec: Vec<u32> = allowed_cats.iter().copied().collect();
    let titles_deleted = delete_titles_by_cat_not_in(conn, &allowed_cats_vec)
        .context("failed to delete titles by cat filter")?;
    if titles_deleted > 0 {
        tracing::info!(
            deleted = titles_deleted,
            "Deleted titles with non-allowed categories"
        );
    }

    let remaining_titles = load_titles(conn).context("failed to load titles after cleanup")?;
    let remaining_tids: Vec<u32> = remaining_titles.iter().map(|t| t.tid).collect();
    let programs_deleted = delete_programs_by_tids_not_in(conn, &remaining_tids)
        .context("failed to delete programs by tid filter")?;
    if programs_deleted > 0 {
        tracing::info!(deleted = programs_deleted, "Deleted orphaned programs");
    }

    Ok(())
}

async fn run_db_sync(args: &DbSyncArgs, dir: Option<&PathBuf>) -> Result<()> {
    let client = build_syoboi_client()?;

    let config_path = resolve_config_path(dir).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;
    let allowed_cats: HashSet<u32> = config.syoboi.titles.cat.iter().copied().collect();
    tracing::info!(?allowed_cats, "Category filter loaded from config");

    let range = resolve_time_range(args.time_since.as_deref(), args.time_until.as_deref())?;
    tracing::info!(
        "Time range: {} .. {}",
        range.start.format("%Y-%m-%d %H:%M:%S"),
        range.end.format("%Y-%m-%d %H:%M:%S"),
    );

    let ch_ids = resolve_ch_ids(args.ch_ids.clone(), dir)?;

    let params = ProgLookupParams {
        ch_ids: Some(ch_ids),
        range: Some(range),
        ..ProgLookupParams::default()
    };

    tracing::info!("Fetching programs from Syoboi API...");
    let programs = lookup_all_programs(&client, &params)
        .await
        .context("failed to fetch programs")?;
    tracing::info!("Fetched {} programs", programs.len());

    // Extract unique TIDs and fetch titles in chunks
    let all_fetched_tids: HashSet<u32> = programs.iter().map(|p| p.tid).collect();
    let unique_tids: Vec<u32> = all_fetched_tids.iter().copied().collect();
    tracing::info!("Fetching titles for {} unique TIDs...", unique_tids.len());

    let all_titles = fetch_titles_chunked(&client, &unique_tids).await?;
    tracing::info!("Fetched {} titles total", all_titles.len());

    // Filter titles by allowed categories
    let filtered_titles: Vec<&SyoboiTitle> = all_titles
        .iter()
        .filter(|t| t.cat.is_some_and(|c| allowed_cats.contains(&c)))
        .collect();
    let cat_filtered = all_titles.len().saturating_sub(filtered_titles.len());
    if cat_filtered > 0 {
        tracing::info!(
            filtered = cat_filtered,
            remaining = filtered_titles.len(),
            "Filtered titles by category"
        );
    }

    // Open DB and upsert
    let data_dir = resolve_data_dir(dir).context("failed to resolve data directory")?;
    let conn = open_db(data_dir.as_ref()).context("failed to open database")?;

    let cached_titles: Vec<CachedTitle> =
        filtered_titles.iter().map(|t| to_cached_title(t)).collect();
    let titles_changed = upsert_titles(&conn, &cached_titles).context("failed to upsert titles")?;
    tracing::info!(
        changed = titles_changed,
        unchanged = cached_titles.len().saturating_sub(titles_changed),
        "Titles upsert complete"
    );

    // Ensure channels referenced by programs exist in DB
    let unique_ch_ids: Vec<u32> = programs
        .iter()
        .map(|p| p.ch_id)
        .collect::<HashSet<u32>>()
        .into_iter()
        .collect();
    tracing::info!(
        "Fetching channels for {} unique ch_ids...",
        unique_ch_ids.len()
    );
    let api_channels = client
        .lookup_channels(Some(&unique_ch_ids))
        .await
        .context("failed to fetch channels")?;
    let cached_channels: Vec<CachedChannel> = api_channels
        .iter()
        .map(|ch| CachedChannel {
            ch_id: ch.ch_id,
            ch_gid: None,
            ch_name: ch.ch_name.clone(),
        })
        .collect();
    let ch_changed =
        upsert_channels(&conn, &cached_channels).context("failed to upsert channels")?;
    tracing::info!(
        fetched = cached_channels.len(),
        changed = ch_changed,
        "Channels upsert complete"
    );

    let valid_tids: HashSet<u32> = cached_titles.iter().map(|t| t.tid).collect();
    let valid_ch_ids: HashSet<u32> = cached_channels.iter().map(|ch| ch.ch_id).collect();
    let (total_programs, programs_changed) = upsert_filtered_programs(
        &conn,
        &programs,
        &valid_tids,
        &valid_ch_ids,
        &all_fetched_tids,
    )?;

    cleanup_disallowed_cats(&conn, &allowed_cats)?;

    tracing::info!(
        "Sync complete: {} titles ({} changed), {} programs ({} changed)",
        cached_titles.len(),
        titles_changed,
        total_programs,
        programs_changed,
    );

    Ok(())
}

/// TMDB Animation genre ID.
const TMDB_GENRE_ANIMATION: u32 = 16;

/// Returns `true` if the Syoboi category requires Animation genre filtering.
const fn requires_animation_filter(cat: Option<u32>) -> bool {
    matches!(cat, Some(1 | 7 | 8 | 10))
}

/// Extracts a base search query from a title using normalization and regex.
fn extract_base_query(title: &str, compiled_regex: Option<&regex::Regex>) -> String {
    let normalized = crate::tui::normalize_viewer::state::normalize_chars(title);

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

/// Compiles `regex_titles` patterns into a single regex joined with `|`.
///
/// Returns `None` if the list is empty or the combined pattern is invalid.
fn compile_regex_titles(patterns: &[String]) -> Option<regex::Regex> {
    if patterns.is_empty() {
        return None;
    }
    let combined = patterns.join("|");
    regex::Regex::new(&combined)
        .map_err(|e| {
            tracing::warn!(pattern = %combined, error = %e, "Failed to compile regex_titles");
        })
        .ok()
}

/// Regex to extract the first number from matched text.
#[allow(clippy::expect_used)]
static FIRST_DIGIT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\d+").expect("digit regex must compile"));

/// General-purpose season number regex for common patterns.
///
/// Matches: `第Nシリーズ`, `第N期`, `第Nクール`, `Season N`, `Nth Season`.
#[allow(clippy::expect_used)]
static GENERAL_SEASON_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?:第(\d+)(?:期|クール|シリーズ)|(?i:season)\s*(\d+)|(\d+)(?:st|nd|rd|th)\s+(?i:season))",
    )
    .expect("general season regex must compile")
});

/// Extracts a season number from a title.
///
/// 1. If the compiled config regex matches, extracts the first digit from
///    the matched portion.
/// 2. Otherwise, falls back to general season patterns (e.g. `第Nシリーズ`,
///    `Season N`).
fn extract_season_number(title: &str, compiled_regex: Option<&regex::Regex>) -> Option<u32> {
    let normalized = crate::tui::normalize_viewer::state::normalize_chars(title);

    // Try config regex first.
    if let Some(re) = compiled_regex
        && let Some(m) = re.find(&normalized)
    {
        let trimmed = m.as_str();
        let result = FIRST_DIGIT_RE
            .find(trimmed)
            .and_then(|d| d.as_str().parse::<u32>().ok());
        if result.is_some() {
            return result;
        }
    }

    // Fallback: general season patterns.
    let caps = GENERAL_SEASON_RE.captures(&normalized)?;
    caps.get(1)
        .or_else(|| caps.get(2))
        .or_else(|| caps.get(3))
        .and_then(|m| m.as_str().parse::<u32>().ok())
}

/// Result of a single TMDB lookup attempt.
enum LookupOutcome {
    /// Successfully matched with TMDB result data and optional (`season_number`, `season_id`).
    Success(u64, String, String, String, Option<(u32, u64)>),
    /// No match found (empty results or filter miss).
    Skipped,
    /// API error occurred.
    Error,
}

/// Resolves expected TMDB media type based on Syoboi category code.
fn resolve_media_type(cat: Option<u32>, cat_movie: &HashSet<u32>) -> TmdbMediaType {
    match cat {
        Some(c) if cat_movie.contains(&c) => TmdbMediaType::Movie,
        _ => TmdbMediaType::Tv,
    }
}

/// Fetches alternative titles and builds a `LookupOutcome::Success`.
async fn fetch_alt_and_build_outcome(
    tmdb_client: &TmdbClient,
    tid: u32,
    media_type: TmdbMediaType,
    tmdb_id: u64,
    original_name: &str,
    name: &str,
) -> Result<LookupOutcome> {
    let alt_titles = match tmdb_client.alternative_titles(media_type, tmdb_id).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(
                tid, tmdb_id, error = %e, "Failed to fetch alternative titles"
            );
            return Ok(LookupOutcome::Error);
        }
    };
    let alt_json =
        serde_json::to_string(&alt_titles.results).context("failed to serialize alt titles")?;
    tracing::info!(
        tid,
        tmdb_id,
        original_name,
        name,
        alt_titles_count = alt_titles.results.len(),
        "TMDB result matched ({media_type:?})"
    );
    Ok(LookupOutcome::Success(
        tmdb_id,
        original_name.to_owned(),
        name.to_owned(),
        alt_json,
        None,
    ))
}

/// Searches TMDB with a query and returns the first matching result.
///
/// Returns `Ok(Some(LookupOutcome::Success(...)))` on match,
/// `Ok(None)` when no result passes filters (caller should try next query),
/// or `Ok(Some(LookupOutcome::Error))` / `Err` on API failure.
async fn search_tmdb_filtered(
    tmdb_client: &TmdbClient,
    query: &str,
    language: &str,
    tid: u32,
    expected_type: TmdbMediaType,
    check_animation: bool,
) -> Result<Option<LookupOutcome>> {
    let mut page = 1u32;

    loop {
        let params = SearchMultiParams::new(query).language(language).page(page);
        let search_result = match tmdb_client.search_multi(&params).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(tid, error = %e, "TMDB search failed");
                return Ok(Some(LookupOutcome::Error));
            }
        };

        if search_result.results.is_empty() {
            return Ok(None);
        }

        for result in &search_result.results {
            match result {
                TmdbMultiSearchResult::Tv(tv) if expected_type == TmdbMediaType::Tv => {
                    let animation =
                        !check_animation || tv.genre_ids.contains(&TMDB_GENRE_ANIMATION);
                    let lang_ja = tv.original_language == "ja";
                    tracing::info!(
                        tid, tmdb_id = tv.id, name = %tv.name,
                        animation, lang_ja,
                        matched = animation, "Filter check (TV)"
                    );
                    if animation {
                        let outcome = fetch_alt_and_build_outcome(
                            tmdb_client,
                            tid,
                            TmdbMediaType::Tv,
                            tv.id,
                            &tv.original_name,
                            &tv.name,
                        )
                        .await?;
                        return Ok(Some(outcome));
                    }
                }
                TmdbMultiSearchResult::Movie(movie) if expected_type == TmdbMediaType::Movie => {
                    let animation =
                        !check_animation || movie.genre_ids.contains(&TMDB_GENRE_ANIMATION);
                    let lang_ja = movie.original_language == "ja";
                    tracing::info!(
                        tid, tmdb_id = movie.id, title = %movie.title,
                        animation, lang_ja,
                        matched = animation, "Filter check (Movie)"
                    );
                    if animation {
                        let outcome = fetch_alt_and_build_outcome(
                            tmdb_client,
                            tid,
                            TmdbMediaType::Movie,
                            movie.id,
                            &movie.original_title,
                            &movie.title,
                        )
                        .await?;
                        return Ok(Some(outcome));
                    }
                }
                _ => {}
            }
        }

        if page >= search_result.total_pages {
            break;
        }
        page = page.saturating_add(1);
    }

    Ok(None)
}

/// Verifies a season number against TMDB `tv_details` and returns the verified
/// `(season_number, season_id)`, or `None` if not applicable / not found.
async fn verify_season_number(
    tmdb_client: &TmdbClient,
    tid: u32,
    tmdb_id: u64,
    language: &str,
    expected_type: TmdbMediaType,
    season_num: Option<u32>,
) -> Option<(u32, u64)> {
    let sn = season_num?;
    if expected_type != TmdbMediaType::Tv {
        return None;
    }
    match tmdb_client.tv_details(tmdb_id, language).await {
        Ok(details) => {
            let season = details.seasons.iter().find(|s| s.season_number == sn);
            let found = season.is_some();
            tracing::info!(tid, tmdb_id, season = sn, found, "Season check");
            season.map(|s| (sn, s.id))
        }
        Err(e) => {
            tracing::warn!(tid, error = %e, "tv_details failed, skipping season");
            None
        }
    }
}

/// Performs TMDB search and filtering for a single title.
///
/// Tries `base_query` first, then falls back to filtered keywords from Syoboi.
/// For TV results, verifies the extracted season number via `tv_details`.
#[instrument(skip_all)]
async fn lookup_single_title(
    title: &CachedTitle,
    tmdb_client: &TmdbClient,
    language: &str,
    compiled_regex: Option<&regex::Regex>,
    cat_movie: &HashSet<u32>,
) -> Result<LookupOutcome> {
    let base_query = extract_base_query(&title.title, compiled_regex);
    let expected_type = resolve_media_type(title.cat, cat_movie);
    let check_animation = requires_animation_filter(title.cat);

    tracing::info!(
        tid = title.tid, title = %title.title, base_query = %base_query,
        media_type = ?expected_type, "Searching TMDB"
    );

    // 1. Try base_query
    if let Some(outcome) = search_tmdb_filtered(
        tmdb_client,
        &base_query,
        language,
        title.tid,
        expected_type,
        check_animation,
    )
    .await?
    {
        if let LookupOutcome::Success(tmdb_id, orig, name, alt, _) = outcome {
            let season_num = extract_season_number(&title.title, compiled_regex);
            let verified = verify_season_number(
                tmdb_client,
                title.tid,
                tmdb_id,
                language,
                expected_type,
                season_num,
            )
            .await;
            return Ok(LookupOutcome::Success(tmdb_id, orig, name, alt, verified));
        }
        return Ok(outcome);
    }

    // 2. Fallback: try filtered keywords
    let keywords =
        dtvmgr_db::filter_keywords(&title.keywords, &title.title, title.short_title.as_deref());
    for kw in &keywords {
        tracing::info!(tid = title.tid, keyword = %kw, "Trying keyword fallback");
        if let Some(outcome) = search_tmdb_filtered(
            tmdb_client,
            kw,
            language,
            title.tid,
            expected_type,
            check_animation,
        )
        .await?
        {
            if let LookupOutcome::Success(tmdb_id, orig, name, alt, _) = outcome {
                let season_num = extract_season_number(&title.title, compiled_regex);
                let verified = verify_season_number(
                    tmdb_client,
                    title.tid,
                    tmdb_id,
                    language,
                    expected_type,
                    season_num,
                )
                .await;
                return Ok(LookupOutcome::Success(tmdb_id, orig, name, alt, verified));
            }
            return Ok(outcome);
        }
    }

    tracing::info!(tid = title.tid, "No match after keyword fallback");
    Ok(LookupOutcome::Skipped)
}

/// Cooldown period (hours) before re-searching a title on TMDB.
const TMDB_LOOKUP_COOLDOWN_HOURS: i64 = 12;

/// Returns `true` if the title was looked up within the cooldown period.
fn is_within_cooldown(tmdb_last_updated: Option<&str>) -> bool {
    let Some(ts) = tmdb_last_updated else {
        return false;
    };
    let Ok(last) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%SZ") else {
        return false;
    };
    let last_utc = last.and_utc();
    let elapsed = Utc::now().signed_duration_since(last_utc);
    elapsed.num_hours() < TMDB_LOOKUP_COOLDOWN_HOURS
}

/// Filters titles based on `--force` and `--retry-unmapped` flags.
///
/// - `force`: return all titles (skip no filtering).
/// - `retry_unmapped`: return only titles with `tmdb_series_id IS NULL` (ignore cooldown).
/// - default: skip titles within the cooldown period.
fn filter_titles(titles: Vec<CachedTitle>, force: bool, retry_unmapped: bool) -> Vec<CachedTitle> {
    if force {
        return titles;
    }
    if retry_unmapped {
        return titles
            .into_iter()
            .filter(|t| t.tmdb_series_id.is_none())
            .collect();
    }
    titles
        .into_iter()
        .filter(|t| !is_within_cooldown(t.tmdb_last_updated.as_deref()))
        .collect()
}

/// Runs the `db tmdb-lookup` subcommand.
///
/// Searches TMDB for cached titles and stores search results
/// (`original_name`, `name`, `alternative_titles`) in the database.
///
/// # Errors
///
/// Returns an error if API calls or DB operations fail.
#[instrument(skip_all)]
#[allow(clippy::too_many_lines)]
async fn run_db_tmdb_lookup(args: &DbTmdbLookupArgs, dir: Option<&PathBuf>) -> Result<()> {
    let data_dir = resolve_data_dir(dir).context("failed to resolve data directory")?;
    let conn = open_db(data_dir.as_ref()).context("failed to open database")?;
    let config_path = resolve_config_path(dir).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;

    let language = resolve_tmdb_language(args.language.as_deref(), dir);
    let tmdb_client = build_tmdb_client(dir)?;

    let mapping_dir = config_path
        .parent()
        .context("failed to get config directory")?;
    let (mut mapping_file, mapping_path) = load_or_fetch(mapping_dir)
        .await
        .context("failed to load mapping file")?;
    let mapping_index = mapping_file.build_index();
    tracing::info!(entries = mapping_index.len(), "Mapping index loaded");

    let excluded_tids: HashSet<u32> = config.syoboi.titles.excludes.iter().copied().collect();

    let titles = if let Some(ref tids) = args.tids {
        let loaded = load_titles_by_tids(&conn, tids).context("failed to load titles by tids")?;
        filter_titles(loaded, args.force, args.retry_unmapped)
    } else {
        let all = load_titles(&conn).context("failed to load titles")?;
        filter_titles(all, args.force, args.retry_unmapped)
    };

    let titles: Vec<_> = titles
        .into_iter()
        .filter(|t| !excluded_tids.contains(&t.tid))
        .collect();

    if titles.is_empty() {
        tracing::info!("No titles to process");
        return Ok(());
    }

    tracing::info!("Processing {} titles...", titles.len());

    let compiled_regex = compile_regex_titles(&config.normalize.regex_titles);

    let cat_movie: HashSet<u32> = config.syoboi.titles.cat_movie.iter().copied().collect();

    let mut success_count: usize = 0;
    let mut skip_count: usize = 0;
    let mut error_count: usize = 0;
    let mut mapped_count: usize = 0;
    let mut needs_template: Vec<(u32, String)> = Vec::new();
    let mut season_id_updates: std::collections::HashMap<u32, u64> =
        std::collections::HashMap::new();

    let total = titles.len();
    #[allow(clippy::as_conversions)]
    let width = total
        .checked_ilog10()
        .map_or(1, |n| (n as usize).saturating_add(1));

    for (i, title) in titles.iter().enumerate() {
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        // Check manual mapping first
        if let Some(entry) = mapping_index.get(&title.tid)
            && entry.tmdb_series_id > 0
        {
            // Resolve tmdb_season_id via API if season_number is set but season_id is missing
            let resolved_season_id =
                if entry.tmdb_season_number.is_some() && entry.tmdb_season_id == 0 {
                    let expected_type = resolve_media_type(title.cat, &cat_movie);
                    let verified = verify_season_number(
                        &tmdb_client,
                        title.tid,
                        entry.tmdb_series_id,
                        &language,
                        expected_type,
                        entry.tmdb_season_number,
                    )
                    .await;
                    if let Some((_, sid)) = verified {
                        season_id_updates.insert(title.tid, sid);
                        Some(sid)
                    } else {
                        None
                    }
                } else if entry.tmdb_season_id > 0 {
                    Some(entry.tmdb_season_id)
                } else {
                    None
                };

            update_tmdb_mapping(
                &conn,
                title.tid,
                Some(entry.tmdb_series_id),
                entry.tmdb_season_number,
                resolved_season_id,
            )
            .with_context(|| format!("failed to apply manual mapping for tid {}", title.tid))?;
            update_tmdb_last_updated(&conn, title.tid, &now).with_context(|| {
                format!("failed to update tmdb_last_updated for tid {}", title.tid)
            })?;
            tracing::info!(
                tid = title.tid,
                tmdb_series_id = entry.tmdb_series_id,
                season = entry.tmdb_season_number,
                season_id = resolved_season_id,
                "Applied manual mapping"
            );
            mapped_count = mapped_count.saturating_add(1);

            let current = i.saturating_add(1);
            #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
            let pct = (current as f64 / total as f64) * 100.0;
            let miss = skip_count.saturating_add(error_count);
            tracing::info!(
                "{current:0>width$}/{total:0>width$} ({pct:06.2}%), match={}, miss={miss}",
                success_count.saturating_add(mapped_count),
            );
            continue;
        }

        match lookup_single_title(
            title,
            &tmdb_client,
            &language,
            compiled_regex.as_ref(),
            &cat_movie,
        )
        .await?
        {
            LookupOutcome::Success(tmdb_id, original_name, name, alt_json, season_info) => {
                update_tmdb_search_result(
                    &conn,
                    title.tid,
                    tmdb_id,
                    &original_name,
                    &name,
                    &alt_json,
                    &now,
                )
                .with_context(|| format!("failed to update TMDB result for tid {}", title.tid))?;
                if let Some((sn, sid)) = season_info {
                    update_tmdb_mapping(&conn, title.tid, Some(tmdb_id), Some(sn), Some(sid))
                        .with_context(|| {
                            format!("failed to update season for tid {}", title.tid)
                        })?;
                    tracing::info!(
                        tid = title.tid,
                        tmdb_id,
                        season = sn,
                        season_id = sid,
                        "Season number saved"
                    );
                }
                tracing::info!(tid = title.tid, tmdb_id, "TMDB result saved");
                success_count = success_count.saturating_add(1);
            }
            LookupOutcome::Skipped => {
                update_tmdb_last_updated(&conn, title.tid, &now).with_context(|| {
                    format!("failed to update tmdb_last_updated for tid {}", title.tid)
                })?;
                skip_count = skip_count.saturating_add(1);
                needs_template.push((title.tid, title.title.clone()));
            }
            LookupOutcome::Error => {
                update_tmdb_last_updated(&conn, title.tid, &now).with_context(|| {
                    format!("failed to update tmdb_last_updated for tid {}", title.tid)
                })?;
                error_count = error_count.saturating_add(1);
                needs_template.push((title.tid, title.title.clone()));
            }
        }

        let current = i.saturating_add(1);
        #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
        let pct = (current as f64 / total as f64) * 100.0;
        let miss = skip_count.saturating_add(error_count);
        tracing::info!(
            "{current:0>width$}/{total:0>width$} ({pct:06.2}%), match={}, miss={miss}",
            success_count.saturating_add(mapped_count),
        );
    }

    tracing::info!(
        total = titles.len(),
        success = success_count,
        skipped = skip_count,
        errors = error_count,
        mapped = mapped_count,
        "TMDB lookup complete"
    );

    // Apply discovered season_id values back to mapping entries
    if !season_id_updates.is_empty() {
        for entry in &mut mapping_file.mappings {
            if let Some(sid) = season_id_updates.get(&entry.tid) {
                entry.tmdb_season_id = *sid;
            }
        }
        tracing::info!(
            updated = season_id_updates.len(),
            "Updated mapping entries with resolved tmdb_season_id"
        );
    }

    // Merge skipped/errored titles into mapping file and save
    let skipped_refs: Vec<(u32, &str)> = needs_template
        .iter()
        .map(|(tid, name)| (*tid, name.as_str()))
        .collect();
    if !skipped_refs.is_empty() {
        mapping_file.merge_new_entries(&skipped_refs);
    }

    // Remove any entries whose tid is in excludes
    let pre_remove = mapping_file.mappings.len();
    mapping_file.remove_excluded(&excluded_tids);
    let removed = pre_remove.saturating_sub(mapping_file.mappings.len());
    if removed > 0 {
        tracing::info!(removed, "Removed excluded TIDs from mapping file");
    }

    if !skipped_refs.is_empty() || removed > 0 || !season_id_updates.is_empty() {
        mapping_file
            .save(&mapping_path)
            .context("failed to save updated mapping file")?;
        tracing::info!(
            new = skipped_refs.len(),
            total = mapping_file.mappings.len(),
            path = %mapping_path.display(),
            "Updated mapping file"
        );
    }

    Ok(())
}

/// Builds a `TmdbClient` from `TMDB_API_TOKEN` env var with config file fallback.
///
/// # Errors
///
/// Returns an error if neither env var nor config `api_key` is set, or the client
/// fails to build.
#[instrument(skip_all)]
fn build_tmdb_client(dir: Option<&PathBuf>) -> Result<TmdbClient> {
    let api_token =
        if let Ok(token) = std::env::var("TMDB_API_TOKEN") {
            token
        } else {
            let config_path = resolve_config_path(dir).context("failed to resolve config path")?;
            let config = AppConfig::load(&config_path).context("failed to load config")?;
            config.tmdb.api.api_key.context(
                "TMDB_API_TOKEN env var is not set and tmdb.api.api_key is not configured",
            )?
        };

    TmdbClient::builder()
        .api_token(api_token)
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("failed to build TMDB client")
}

/// Resolves TMDB language: CLI arg > config > "en-US".
fn resolve_tmdb_language(cli_lang: Option<&str>, dir: Option<&PathBuf>) -> String {
    if let Some(lang) = cli_lang {
        return lang.to_owned();
    }
    if let Ok(config_path) = resolve_config_path(dir)
        && let Ok(config) = AppConfig::load(&config_path)
        && let Some(lang) = config.tmdb.language
    {
        return lang;
    }
    String::from("en-US")
}

/// Runs the `tmdb search-tv` subcommand (internally uses `search/multi`).
///
/// # Errors
///
/// Returns an error if the TMDB client fails to build or the API request fails.
#[instrument(skip_all)]
async fn run_tmdb_search_tv(args: &TmdbSearchTvArgs, dir: Option<&PathBuf>) -> Result<()> {
    let client = build_tmdb_client(dir)?;
    let language = resolve_tmdb_language(args.language.as_deref(), dir);

    let params = SearchMultiParams::new(&args.query).language(&language);
    let response = client
        .search_multi(&params)
        .await
        .context("TMDB search/multi request failed")?;

    tracing::info!("Total results: {}", response.total_results);
    tracing::info!("ID\tName\t\t\tOrigLang\tCountry\t\tFirstAirDate");
    for result in &response.results {
        if let TmdbMultiSearchResult::Tv(tv) = result {
            tracing::info!(
                "{}\t\t{}\t{}\t\t{}\t\t{}",
                tv.id,
                tv.name,
                tv.original_language,
                tv.origin_country.join(","),
                tv.first_air_date.as_deref().unwrap_or("-"),
            );
        }
    }

    Ok(())
}

/// Runs the `tmdb search-movie` subcommand (internally uses `search/multi`).
///
/// # Errors
///
/// Returns an error if the TMDB client fails to build or the API request fails.
#[instrument(skip_all)]
async fn run_tmdb_search_movie(args: &TmdbSearchMovieArgs, dir: Option<&PathBuf>) -> Result<()> {
    let client = build_tmdb_client(dir)?;
    let language = resolve_tmdb_language(args.language.as_deref(), dir);

    let params = SearchMultiParams::new(&args.query).language(&language);
    let response = client
        .search_multi(&params)
        .await
        .context("TMDB search/multi request failed")?;

    tracing::info!("Total results: {}", response.total_results);
    tracing::info!("ID\tTitle\t\t\tOrigLang\tReleaseDate");
    for result in &response.results {
        if let TmdbMultiSearchResult::Movie(movie) = result {
            tracing::info!(
                "{}\t{}\t{}\t\t{}",
                movie.id,
                movie.title,
                movie.original_language,
                movie.release_date.as_deref().unwrap_or("-"),
            );
        }
    }

    Ok(())
}

/// Runs the `tmdb tv-details` subcommand.
///
/// # Errors
///
/// Returns an error if the TMDB client fails to build or the API request fails.
#[instrument(skip_all)]
async fn run_tmdb_tv_details(args: &TmdbTvDetailsArgs, dir: Option<&PathBuf>) -> Result<()> {
    let client = build_tmdb_client(dir)?;
    let language = resolve_tmdb_language(args.language.as_deref(), dir);

    let details = client
        .tv_details(args.id, &language)
        .await
        .context("TMDB tv details request failed")?;

    tracing::info!("ID: {}", details.id);
    tracing::info!("Name: {}", details.name);
    tracing::info!("Original Name: {}", details.original_name);
    tracing::info!(
        "First Air Date: {}",
        details.first_air_date.as_deref().unwrap_or("-")
    );
    tracing::info!("Status: {}", details.status.as_deref().unwrap_or("-"));
    tracing::info!("Seasons: {}", details.number_of_seasons);
    tracing::info!("Episodes: {}", details.number_of_episodes);
    tracing::info!("---");
    for season in &details.seasons {
        tracing::info!(
            "  Season {}: {} episodes (air_date: {})",
            season.season_number,
            season.episode_count,
            season.air_date.as_deref().unwrap_or("-"),
        );
    }

    Ok(())
}

/// Runs the `tmdb tv-season` subcommand.
///
/// # Errors
///
/// Returns an error if the TMDB client fails to build or the API request fails.
#[instrument(skip_all)]
async fn run_tmdb_tv_season(args: &TmdbTvSeasonArgs, dir: Option<&PathBuf>) -> Result<()> {
    let client = build_tmdb_client(dir)?;
    let language = resolve_tmdb_language(args.language.as_deref(), dir);

    let season = client
        .tv_season(args.id, args.season, &language)
        .await
        .context("TMDB tv season request failed")?;

    tracing::info!(
        "Season {}: {}",
        season.season_number,
        season.name.as_deref().unwrap_or("-")
    );
    tracing::info!("Episodes:");
    for ep in &season.episodes {
        tracing::info!(
            "  E{:02}: {} (air_date: {}, runtime: {}min)",
            ep.episode_number,
            ep.name,
            ep.air_date.as_deref().unwrap_or("-"),
            ep.runtime
                .map_or_else(|| String::from("-"), |r| r.to_string()),
        );
    }

    Ok(())
}

// ── jlse subcommands ──────────────────────────────────────────

/// Resolves the `JlseConfig` from the app config.
///
/// # Errors
///
/// Returns an error if the jlse section is not configured.
fn resolve_jlse_config(dir: Option<&PathBuf>) -> Result<JlseConfig> {
    let config_path = resolve_config_path(dir).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;
    config
        .jlse
        .context("jlse config not found in dtvmgr.toml; add [jlse.dirs] with jl, logo, result")
}

/// Environment variable name for channel name override.
///
/// The original Node.js tool uses `CHNNELNAME` (intentional typo preserved).
const CHANNEL_ENV_VAR: &str = "CHNNELNAME";

/// Resolve channel name from CLI argument or environment variable.
fn resolve_channel_name(arg: Option<&str>) -> Option<String> {
    arg.map(ToOwned::to_owned)
        .or_else(|| std::env::var(CHANNEL_ENV_VAR).ok())
}

/// Detect the recording target from the middle of a TS file.
///
/// Thin wrapper around [`dtvmgr_tsduck::detect_target_from_middle`] that
/// discards the raw XML.
fn detect_target_from_middle(
    bin: &Path,
    input: &Path,
) -> Result<Option<dtvmgr_tsduck::eit::RecordingTarget>> {
    let (target, _xml) = dtvmgr_tsduck::detect_target_from_middle(bin, input)?;
    Ok(target)
}

/// Runs the `jlse tsduck` subcommand.
///
/// Extracts EIT program information from a TS file using `TSDuck`.
/// When `--sid` or `-c` is given, filters by that service ID using EIT-only
/// extraction. Otherwise, extracts all tables (PAT + EIT) and auto-detects
/// the recording target from PAT's first service ID.
///
/// # Errors
///
/// Returns an error if `TSDuck` fails or the EIT XML cannot be parsed.
#[allow(clippy::print_stdout)]
#[instrument(skip_all)]
fn run_jlse_tsduck(args: &JlseTsduckArgs, dir: Option<&PathBuf>) -> Result<()> {
    // --sid takes priority over -c (channel name detection).
    let explicit_sid = if let Some(ref sid) = args.sid {
        Some(sid.clone())
    } else if args.channel.is_some() {
        let jlse_config = resolve_jlse_config(dir)?;
        let data = DataPaths::from_config(&jlse_config);
        let channels = load_jlse_channels(&data.channel_list)?;
        let channel_name = resolve_channel_name(args.channel.as_deref());
        let filepath = args.input.to_string_lossy();
        let ch = detect_channel(&channels, &filepath, channel_name.as_deref());
        if let Some(ref ch) = ch {
            println!("=== Channel Detection ===");
            println!("short: {}", ch.short);
            println!("service_id: {}", ch.service_id);
            println!();
        }
        ch.map(|c| c.service_id)
    } else {
        None
    };

    let programs = if let Some(ref sid) = explicit_sid {
        // Explicit SID: extract EIT only (PID 0x12).
        let xml = dtvmgr_tsduck::command::extract_eit(&args.tstables_bin, &args.input)?;
        println!("=== EIT Program Information (SID: {sid}) ===");
        dtvmgr_tsduck::eit::parse_eit_xml_by_sid(&xml, sid)
            .with_context(|| format!("failed to parse EIT XML for SID {sid}"))?
    } else {
        // No explicit SID: extract PAT (PID 0) and EIT (PID 0x12) separately.
        let pat_xml = dtvmgr_tsduck::command::extract_pat(&args.tstables_bin, &args.input)?;
        let pat_sid = dtvmgr_tsduck::pat::parse_pat_first_service_id(&pat_xml)
            .context("failed to parse PAT XML")?;

        let eit_xml = dtvmgr_tsduck::command::extract_eit(&args.tstables_bin, &args.input)?;
        if let Some(sid) = pat_sid {
            println!("=== EIT Program Information (SID: {sid} from PAT) ===");
            let all =
                dtvmgr_tsduck::eit::parse_eit_xml(&eit_xml).context("failed to parse EIT XML")?;
            all.into_iter().filter(|p| p.service_id == sid).collect()
        } else {
            println!("=== EIT Program Information ===");
            dtvmgr_tsduck::eit::parse_eit_xml(&eit_xml).context("failed to parse EIT XML")?
        }
    };

    let programs = dtvmgr_tsduck::eit::dedup_programs(programs);

    // Detect recording target from middle-of-file EIT p/f.
    let recording_target = detect_target_from_middle(&args.tstables_bin, &args.input);
    let target_event_id = match &recording_target {
        Ok(Some(target)) => {
            println!(
                "=== Recording Target: event_id={} ({:?}) ===",
                target.program.event_id, target.detection_method
            );
            println!();
            Some(target.program.event_id.clone())
        }
        Ok(None) => {
            println!("=== Recording Target: not detected ===");
            println!();
            None
        }
        Err(e) => {
            println!("=== Recording Target: detection failed ({e:#}) ===");
            println!();
            None
        }
    };

    for p in &programs {
        print_program_info(p, target_event_id.as_deref());
    }
    Ok(())
}

/// Print a single program's information to stdout.
#[allow(clippy::print_stdout)]
fn print_program_info(p: &dtvmgr_tsduck::eit::ProgramInfo, target_event_id: Option<&str>) {
    let marker = if target_event_id == Some(&p.event_id) {
        "[recording_target] "
    } else {
        ""
    };
    println!("--- {marker}event_id: {} ---", p.event_id);
    println!("  service_id: {}", p.service_id);
    println!("  start_time: {}", p.start_time);
    println!(
        "  duration: {} ({} min / {} sec)",
        p.duration_raw,
        p.duration_min(),
        p.duration_sec
    );
    println!("  running_status: {}", p.running_status);
    if let Some(name) = &p.program_name {
        println!("  program_name: {name}");
    }
    if let Some(desc) = &p.description {
        println!("  description: {desc}");
    }
    if let Some(tt) = &p.table_type {
        println!("  table_type: {tt}");
    }
    if let Some(ext) = p.extended() {
        println!("  extended: {ext}");
    }
    if !p.raw_extended.is_empty() {
        for (key, value) in &p.raw_extended {
            println!("  raw_extended[{key}]: {value}");
        }
    }
    if let Some(g) = p.genre1 {
        println!("  genre: {g}/{}", p.sub_genre1.unwrap_or(0));
    }
    if let Some(r) = p.video_resolution() {
        println!("  video: {r}");
    }
    if let Some(rate) = p.audio_sampling_rate() {
        println!("  audio: {rate}Hz");
    }
    println!();
}

/// Runs the `jlse channel` subcommand.
///
/// Detects broadcast channel from the input filename.
///
/// # Errors
///
/// Returns an error if the channel list cannot be loaded.
#[allow(clippy::print_stdout)]
#[instrument(skip_all)]
fn run_jlse_channel(args: &JlseChannelArgs, dir: Option<&PathBuf>) -> Result<()> {
    let jlse_config = resolve_jlse_config(dir)?;
    let data = DataPaths::from_config(&jlse_config);
    let channels = load_jlse_channels(&data.channel_list)?;

    let channel_name = resolve_channel_name(args.channel.as_deref());

    let filepath = args.input.to_string_lossy();
    let result = detect_channel(&channels, &filepath, channel_name.as_deref());

    match result {
        Some(ch) => {
            println!("recognize: {}", ch.recognize);
            println!("install: {}", ch.install);
            println!("short: {}", ch.short);
            println!("service_id: {}", ch.service_id);
        }
        None => {
            println!("No channel detected.");
        }
    }
    Ok(())
}

/// Runs the `jlse param` subcommand.
///
/// Detects JL parameters from the channel and input filename.
///
/// # Errors
///
/// Returns an error if the param lists cannot be loaded.
#[allow(clippy::print_stdout)]
#[instrument(skip_all)]
fn run_jlse_param(args: &JlseParamArgs, dir: Option<&PathBuf>) -> Result<()> {
    let jlse_config = resolve_jlse_config(dir)?;
    let data = DataPaths::from_config(&jlse_config);
    let channels = load_jlse_channels(&data.channel_list)?;

    let channel_name = resolve_channel_name(args.channel.as_deref());
    let filepath = args.input.to_string_lossy();
    let channel = detect_channel(&channels, &filepath, channel_name.as_deref());

    let params_jl1 = load_params(&data.param_jl1)?;
    let params_jl2 = load_params(&data.param_jl2)?;

    let filename = args.input.file_stem().unwrap_or_default().to_string_lossy();
    let result = detect_param(&params_jl1, &params_jl2, channel.as_ref(), &filename);

    println!("jl_run: {}", result.jl_run);
    println!("flags: {}", result.flags);
    println!("options: {}", result.options);
    Ok(())
}

/// Runs the `jlse run` subcommand.
///
/// Executes the full CM detection pipeline.
///
/// # Errors
///
/// Returns an error if the pipeline fails.
#[instrument(skip_all)]
fn run_jlse_run(args: &JlseRunArgs, dir: Option<&PathBuf>) -> Result<()> {
    let mut jlse_config = resolve_jlse_config(dir)?;
    jlse_config.bins.tstables = Some(args.tstables_bin.clone());

    let channel_name = resolve_channel_name(args.channel.as_deref());

    if args.epgstation {
        // EPGStation mode: read INPUT/OUTPUT from environment variables.
        let input = args
            .input
            .clone()
            .or_else(|| std::env::var("INPUT").ok().map(PathBuf::from))
            .context("INPUT environment variable is required in --epgstation mode")?;

        let (out_dir, out_name) = std::env::var("OUTPUT").map_or_else(
            |_| (args.outdir.clone(), args.outname.clone()),
            |output_str| {
                let output_path = PathBuf::from(&output_str);
                let dir = output_path.parent().map(Path::to_path_buf);
                let name = output_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned());
                (dir, name)
            },
        );

        let ctx = PipelineContext {
            input,
            channel_name,
            config: jlse_config,
            filter: args.filter,
            encode: true, // implicitly enabled
            target: AvsTarget::from(args.target),
            add_chapter: args.add_chapter,
            ffmpeg_option: args.ffmpeg_option.clone(),
            out_dir,
            out_name,
            remove: args.remove,
            progress_mode: Some(ProgressMode::EpgStation),
        };

        run_pipeline(&ctx)
    } else {
        let input = args.input.clone().context("--input is required")?;

        let ctx = PipelineContext {
            input,
            channel_name,
            config: jlse_config,
            filter: args.filter,
            encode: args.encode,
            target: AvsTarget::from(args.target),
            add_chapter: args.add_chapter,
            ffmpeg_option: args.ffmpeg_option.clone(),
            out_dir: args.outdir.clone(),
            out_name: args.outname.clone(),
            remove: args.remove,
            progress_mode: None,
        };

        run_pipeline(&ctx)
    }
}

// ── Syoboi / TMDB helpers ────────────────────────────────────

/// Builds a `SyoboiClient` with default user agent.
///
/// # Errors
///
/// Returns an error if the client fails to build.
#[instrument(skip_all)]
fn build_syoboi_client() -> Result<SyoboiClient> {
    SyoboiClient::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("failed to build Syoboi API client")
}

/// Runs the `syoboi channels select` subcommand.
///
/// Fetches channels/groups from API, caches in DB, launches TUI,
/// and saves selection to `dtvmgr.toml`.
///
/// # Errors
///
/// Returns an error if API calls, DB operations, or TUI fails.
#[instrument(skip_all)]
async fn run_channels_select(dir: Option<&PathBuf>) -> Result<()> {
    let client = build_syoboi_client()?;

    tracing::info!("Fetching channel groups from API...");
    let api_groups = client
        .lookup_channel_groups(None)
        .await
        .context("failed to fetch channel groups")?;

    tracing::info!("Fetching channels from API...");
    let api_channels = client
        .lookup_channels(None)
        .await
        .context("failed to fetch channels")?;

    // Cache in DB
    let data_dir = resolve_data_dir(dir).context("failed to resolve data directory")?;
    let conn = open_db(data_dir.as_ref()).context("failed to open database")?;

    let cached_groups: Vec<CachedChannelGroup> = api_groups
        .iter()
        .map(|g| CachedChannelGroup {
            ch_gid: g.ch_gid,
            ch_group_name: g.ch_group_name.clone(),
            ch_group_order: g.ch_group_order,
        })
        .collect();
    let groups_changed =
        upsert_channel_groups(&conn, &cached_groups).context("failed to cache channel groups")?;
    tracing::info!(changed = groups_changed, "Channel groups upsert complete");

    let valid_ch_gids: HashSet<u32> = cached_groups.iter().map(|g| g.ch_gid).collect();
    let cached_channels: Vec<CachedChannel> = api_channels
        .iter()
        .map(|ch| CachedChannel {
            ch_id: ch.ch_id,
            ch_gid: ch.ch_gid.filter(|gid| valid_ch_gids.contains(gid)),
            ch_name: ch.ch_name.clone(),
        })
        .collect();
    let channels_changed =
        upsert_channels(&conn, &cached_channels).context("failed to cache channels")?;
    tracing::info!(changed = channels_changed, "Channels upsert complete");

    // Load config
    let config_path = resolve_config_path(dir).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;
    let initial_selected: BTreeSet<u32> = config.syoboi.channels.selected.into_iter().collect();

    // Build TUI data model
    let groups = build_tui_groups(&cached_groups, &cached_channels);

    tracing::info!(
        "Loaded {} groups, {} channels. Launching TUI...",
        cached_groups.len(),
        cached_channels.len()
    );

    // Run TUI (blocking)
    let result =
        run_channel_selector(groups, initial_selected).context("channel selector TUI failed")?;

    if let Some(selected) = result {
        let mut config = AppConfig::load(&config_path).unwrap_or_default();
        config.syoboi.channels.selected = selected;
        config.save(&config_path).context("failed to save config")?;
        tracing::info!(
            "Saved {} selected channel(s) to {}",
            config.syoboi.channels.selected.len(),
            config_path.display()
        );
    } else {
        tracing::info!("Selection cancelled");
    }

    Ok(())
}

/// Builds TUI channel groups from cached data.
fn build_tui_groups(
    groups: &[CachedChannelGroup],
    channels: &[CachedChannel],
) -> Vec<ChannelGroup> {
    let mut tui_groups: Vec<ChannelGroup> = groups
        .iter()
        .map(|g| ChannelGroup {
            ch_gid: g.ch_gid,
            name: g.ch_group_name.clone(),
            channels: Vec::new(),
        })
        .collect();

    // Sort by ch_group_order (already sorted from DB, but ensure)
    tui_groups.sort_by_key(|g| {
        groups
            .iter()
            .find(|cg| cg.ch_gid == g.ch_gid)
            .map_or(0, |cg| cg.ch_group_order)
    });

    for ch in channels {
        if let Some(ch_gid) = ch.ch_gid
            && let Some(group) = tui_groups.iter_mut().find(|g| g.ch_gid == ch_gid)
        {
            group.channels.push(ChannelEntry {
                ch_id: ch.ch_id,
                ch_name: ch.ch_name.clone(),
            });
        }
    }

    // Sort channels within each group by ch_id
    for group in &mut tui_groups {
        group.channels.sort_by_key(|ch| ch.ch_id);
    }

    tui_groups
}

/// Runs the `syoboi channels list` subcommand.
///
/// # Errors
///
/// Returns an error if config or DB operations fail.
#[instrument(skip_all)]
fn run_channels_list(dir: Option<&PathBuf>) -> Result<()> {
    let config_path = resolve_config_path(dir).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;

    if config.syoboi.channels.selected.is_empty() {
        tracing::info!("No channels selected. Run `syoboi channels select` to choose channels.");
        return Ok(());
    }

    // Try to load names from DB cache
    let data_dir = resolve_data_dir(dir).ok().flatten();
    let conn = open_db(data_dir.as_ref());
    let cached_channels = conn
        .as_ref()
        .ok()
        .and_then(|c| load_channels(c).ok())
        .unwrap_or_default();

    tracing::info!(
        "Selected channels ({}):",
        config.syoboi.channels.selected.len()
    );
    for ch_id in &config.syoboi.channels.selected {
        let name = cached_channels
            .iter()
            .find(|c| c.ch_id == *ch_id)
            .map_or("(unknown)", |c| c.ch_name.as_str());
        tracing::info!("  {:>3}  {}", ch_id, name);
    }

    Ok(())
}

/// Runs the `db list` subcommand.
///
/// Loads titles, programs, and channels from local DB and launches the TUI viewer.
///
/// # Errors
///
/// Returns an error if DB operations or TUI fails.
#[instrument(skip_all)]
fn run_db_list(dir: Option<&PathBuf>) -> Result<()> {
    let data_dir = resolve_data_dir(dir).context("failed to resolve data directory")?;
    let conn = open_db(data_dir.as_ref()).context("failed to open database")?;
    let config_path = resolve_config_path(dir).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;

    let excluded_tids: std::collections::HashSet<u32> =
        config.syoboi.titles.excludes.iter().copied().collect();

    let titles = load_titles(&conn).context("failed to load titles")?;
    let programs = load_programs(&conn).context("failed to load programs")?;
    let channels = load_channels(&conn).context("failed to load channels")?;

    if titles.is_empty() {
        tracing::info!("No titles in database. Run `db sync` first.");
        return Ok(());
    }

    let compiled_regex = compile_regex_titles(&config.normalize.regex_titles);

    tracing::info!(
        "Loaded {} titles, {} programs, {} channels. Launching TUI...",
        titles.len(),
        programs.len(),
        channels.len()
    );

    let output = crate::tui::title_viewer::run_title_viewer(
        &titles,
        &programs,
        channels,
        excluded_tids,
        compiled_regex.as_ref(),
    )
    .context("title viewer TUI failed")?;

    if !output.new_excludes.is_empty() {
        // Reload config to merge with any concurrent changes
        let mut config = AppConfig::load(&config_path).context("failed to reload config")?;
        let mut excludes: std::collections::HashSet<u32> =
            config.syoboi.titles.excludes.drain(..).collect();
        excludes.extend(&output.new_excludes);
        config.syoboi.titles.excludes = excludes.into_iter().collect();
        config.save(&config_path).context("failed to save config")?;
        tracing::info!(
            "Added {} TIDs to excludes (total: {})",
            output.new_excludes.len(),
            config.syoboi.titles.excludes.len(),
        );
    }

    Ok(())
}

/// Runs the `db normalize` subcommand.
///
/// Loads titles from local DB and regex history from config, launches the
/// normalize viewer TUI, saves updated history back to config, and prints
/// selected rows as TSV on quit.
///
/// # Errors
///
/// Returns an error if DB operations, config I/O, or TUI fails.
#[instrument(skip_all)]
fn run_db_normalize(dir: Option<&PathBuf>) -> Result<()> {
    let data_dir = resolve_data_dir(dir).context("failed to resolve data directory")?;
    let conn = open_db(data_dir.as_ref()).context("failed to open database")?;
    let config_path = resolve_config_path(dir).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;

    let titles = load_titles(&conn).context("failed to load titles")?;

    if titles.is_empty() {
        tracing::info!("No titles in database. Run `db sync` first.");
        return Ok(());
    }

    tracing::info!(
        "Loaded {} titles. Launching normalize viewer...",
        titles.len(),
    );

    let (output, updated_history) = crate::tui::normalize_viewer::run_normalize_viewer(
        &titles,
        &config.normalize.regex_history,
        &config.normalize.regex_titles,
    )
    .context("normalize viewer TUI failed")?;

    // Save updated regex history back to config
    let mut config = config;
    config.normalize.regex_history = updated_history;
    config.save(&config_path).context("failed to save config")?;

    #[allow(clippy::print_stdout)]
    if output.len() > 1 {
        for line in &output {
            println!("{line}");
        }
    }

    Ok(())
}

/// Entry point.
///
/// # Errors
///
/// Returns an error if subcommand execution fails.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    #[cfg(not(feature = "otel"))]
    {
        fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_target(false)
            .init();
    }

    #[cfg(feature = "otel")]
    {
        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        let fmt_layer = tracing_subscriber::fmt::layer().with_target(false);

        let otel_layer = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .and_then(|_| {
                let exporter = opentelemetry_otlp::SpanExporter::builder()
                    .with_http()
                    .build()
                    .ok()?;

                let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                    .with_simple_exporter(exporter)
                    .build();

                let tracer = opentelemetry::trace::TracerProvider::tracer(
                    &tracer_provider,
                    env!("CARGO_PKG_NAME"),
                );
                opentelemetry::global::set_tracer_provider(tracer_provider);

                Some(tracing_opentelemetry::layer().with_tracer(tracer))
            });

        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(otel_layer)
            .init();
    }

    let cli = Cli::parse();
    match cli.command {
        Commands::Syoboi(cmd) => match cmd.command {
            SyoboiSubcommands::Prog(args) => run_syoboi_prog(&args, cli.dir.as_ref()).await,
            SyoboiSubcommands::Titles(args) => run_syoboi_titles(&args).await,
            SyoboiSubcommands::Channels(ch) => match ch.command {
                ChannelsSubcommands::Select => run_channels_select(cli.dir.as_ref()).await,
                ChannelsSubcommands::List => run_channels_list(cli.dir.as_ref()),
            },
        },
        Commands::Tmdb(tmdb) => match tmdb.command {
            TmdbSubcommands::SearchTv(args) => run_tmdb_search_tv(&args, cli.dir.as_ref()).await,
            TmdbSubcommands::SearchMovie(args) => {
                run_tmdb_search_movie(&args, cli.dir.as_ref()).await
            }
            TmdbSubcommands::TvDetails(args) => run_tmdb_tv_details(&args, cli.dir.as_ref()).await,
            TmdbSubcommands::TvSeason(args) => run_tmdb_tv_season(&args, cli.dir.as_ref()).await,
        },
        Commands::Db(db) => match db.command {
            DbSubcommands::Sync(args) => run_db_sync(&args, cli.dir.as_ref()).await,
            DbSubcommands::List => run_db_list(cli.dir.as_ref()),
            DbSubcommands::Normalize => run_db_normalize(cli.dir.as_ref()),
            DbSubcommands::TmdbLookup(args) => run_db_tmdb_lookup(&args, cli.dir.as_ref()).await,
        },
        Commands::Jlse(jlse) => match jlse.command {
            JlseSubcommands::Channel(args) => run_jlse_channel(&args, cli.dir.as_ref()),
            JlseSubcommands::Param(args) => run_jlse_param(&args, cli.dir.as_ref()),
            JlseSubcommands::Run(args) => run_jlse_run(&args, cli.dir.as_ref()),
            JlseSubcommands::Tsduck(args) => run_jlse_tsduck(&args, cli.dir.as_ref()),
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::undocumented_unsafe_blocks,
        clippy::indexing_slicing
    )]

    use super::*;

    #[test]
    fn test_compile_regex_titles_empty() {
        // Arrange
        let patterns: Vec<String> = vec![];

        // Act
        let result = compile_regex_titles(&patterns);

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn test_compile_regex_titles_single() {
        // Arrange
        let patterns = vec![String::from(r"第\d+期$")];

        // Act
        let result = compile_regex_titles(&patterns);

        // Assert
        let re = result.unwrap();
        assert!(re.is_match("タイトル 第2期"));
        assert!(!re.is_match("タイトル"));
    }

    #[test]
    fn test_compile_regex_titles_multiple() {
        // Arrange
        let patterns = vec![String::from(r"第\d+期$"), String::from(r"\s*Season\s*\d+")];

        // Act
        let result = compile_regex_titles(&patterns);

        // Assert
        let re = result.unwrap();
        assert!(re.is_match("タイトル 第2期"));
        assert!(re.is_match("Title Season 3"));
        assert!(!re.is_match("タイトル"));
    }

    #[test]
    fn test_compile_regex_titles_invalid() {
        // Arrange
        let patterns = vec![String::from(r"(unclosed")];

        // Act
        let result = compile_regex_titles(&patterns);

        // Assert
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_season_number_english() {
        // Arrange
        let re = regex::Regex::new(r"(?i:\s*season\s*\d+)").unwrap();

        // Act & Assert
        assert_eq!(extract_season_number("作品名 Season 3", Some(&re)), Some(3));
    }

    #[test]
    fn test_extract_season_number_japanese() {
        // Arrange
        let re = regex::Regex::new(r"\s*\(第\d+(?:期|クール)\)").unwrap();

        // Act & Assert
        assert_eq!(extract_season_number("作品名(第2期)", Some(&re)), Some(2));
    }

    #[test]
    fn test_extract_season_number_final_season() {
        // Arrange: "FINAL SEASON" has no digits
        let re = regex::Regex::new(r"(?i:\s*FINAL\s+SEASON)").unwrap();

        // Act & Assert
        assert_eq!(
            extract_season_number("作品名 FINAL SEASON", Some(&re)),
            None
        );
    }

    #[test]
    fn test_extract_season_number_no_regex_fallback() {
        // Act & Assert: no compiled regex, but general pattern matches
        assert_eq!(extract_season_number("作品名 Season 3", None), Some(3));
    }

    #[test]
    fn test_extract_season_number_no_match() {
        // Arrange
        let re = regex::Regex::new(r"(?i:\s*season\s*\d+)").unwrap();

        // Act & Assert: title doesn't match either regex or general pattern
        assert_eq!(extract_season_number("葬送のフリーレン", Some(&re)), None);
    }

    #[test]
    fn test_extract_season_number_series_fallback() {
        // Arrange: config regex only covers 期/クール, not シリーズ
        let re = regex::Regex::new(r"\s*\(第\d+(?:期|クール)\)").unwrap();

        // Act & Assert: general pattern catches 第2シリーズ
        assert_eq!(
            extract_season_number("科学×冒険サバイバル！(第2シリーズ)", Some(&re)),
            Some(2)
        );
    }

    #[test]
    fn test_extract_season_number_no_regex_no_pattern() {
        // Act & Assert: no regex, no general pattern match
        assert_eq!(extract_season_number("葬送のフリーレン", None), None);
    }

    #[test]
    fn test_extract_season_number_nth_season() {
        // Act & Assert: "2nd Season" pattern
        assert_eq!(
            extract_season_number("BanG Dream! 2nd Season", None),
            Some(2)
        );
    }

    #[test]
    fn test_is_within_cooldown_none() {
        // Arrange & Act & Assert: None means never looked up
        assert!(!is_within_cooldown(None));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_is_within_cooldown_recent() {
        // Arrange: 10 hours ago (within 12h cooldown)
        let ts = (Utc::now() - chrono::Duration::hours(10))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();

        // Act & Assert
        assert!(is_within_cooldown(Some(&ts)));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_is_within_cooldown_expired() {
        // Arrange: 13 hours ago (past 12h cooldown)
        let ts = (Utc::now() - chrono::Duration::hours(13))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();

        // Act & Assert
        assert!(!is_within_cooldown(Some(&ts)));
    }

    #[test]
    fn test_is_within_cooldown_invalid_format() {
        // Arrange & Act & Assert: invalid format returns false
        assert!(!is_within_cooldown(Some("not-a-date")));
    }

    fn make_cached_title(
        tid: u32,
        tmdb_series_id: Option<u64>,
        tmdb_last_updated: Option<&str>,
    ) -> CachedTitle {
        CachedTitle {
            tid,
            tmdb_series_id,
            tmdb_season_number: None,
            tmdb_season_id: None,
            title: format!("Title {tid}"),
            short_title: None,
            title_yomi: None,
            title_en: None,
            cat: None,
            title_flag: None,
            first_year: None,
            first_month: None,
            keywords: vec![],
            sub_titles: None,
            last_update: String::new(),
            tmdb_original_name: None,
            tmdb_name: None,
            tmdb_alt_titles: None,
            tmdb_last_updated: tmdb_last_updated.map(String::from),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_filter_titles_default() {
        // Arrange: one title within cooldown, one outside
        let recent_ts = (Utc::now() - chrono::Duration::hours(6))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let old_ts = (Utc::now() - chrono::Duration::hours(24))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let titles = vec![
            make_cached_title(1, Some(100), Some(&recent_ts)),
            make_cached_title(2, None, Some(&old_ts)),
            make_cached_title(3, None, None),
        ];

        // Act
        let result = filter_titles(titles, false, false);

        // Assert: only title 2 (expired cooldown) and 3 (never looked up)
        let tids: Vec<u32> = result.iter().map(|t| t.tid).collect();
        assert_eq!(tids, vec![2, 3]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_filter_titles_force() {
        // Arrange
        let recent_ts = (Utc::now() - chrono::Duration::hours(6))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let titles = vec![
            make_cached_title(1, Some(100), Some(&recent_ts)),
            make_cached_title(2, None, Some(&recent_ts)),
            make_cached_title(3, None, None),
        ];

        // Act
        let result = filter_titles(titles, true, false);

        // Assert: all titles returned
        let tids: Vec<u32> = result.iter().map(|t| t.tid).collect();
        assert_eq!(tids, vec![1, 2, 3]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_filter_titles_retry_unmapped() {
        // Arrange: title 1 is mapped, titles 2 and 3 are unmapped
        let recent_ts = (Utc::now() - chrono::Duration::hours(6))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let titles = vec![
            make_cached_title(1, Some(100), Some(&recent_ts)),
            make_cached_title(2, None, Some(&recent_ts)),
            make_cached_title(3, None, None),
        ];

        // Act
        let result = filter_titles(titles, false, true);

        // Assert: only unmapped titles (2, 3), even though 2 is within cooldown
        let tids: Vec<u32> = result.iter().map(|t| t.tid).collect();
        assert_eq!(tids, vec![2, 3]);
    }

    // ── to_cached_title / to_cached_program ────────────────────

    fn make_syoboi_title(tid: u32) -> SyoboiTitle {
        SyoboiTitle {
            tid,
            last_update: "2024-01-01T00:00:00Z".to_owned(),
            title: format!("Title {tid}"),
            short_title: Some("Short".to_owned()),
            title_yomi: Some("Yomi".to_owned()),
            title_en: Some("English".to_owned()),
            comment: None,
            cat: Some(1),
            title_flag: Some(0),
            first_year: Some(2024),
            first_month: Some(1),
            first_end_year: None,
            first_end_month: None,
            first_ch: None,
            keywords: Some("key1,key2".to_owned()),
            user_point: None,
            user_point_rank: None,
            sub_titles: Some("*01*EP1\r\n*02*EP2".to_owned()),
        }
    }

    #[test]
    fn test_to_cached_title_maps_all_fields() {
        // Arrange
        let src = make_syoboi_title(42);

        // Act
        let ct = to_cached_title(&src);

        // Assert
        assert_eq!(ct.tid, 42);
        assert_eq!(ct.title, "Title 42");
        assert_eq!(ct.short_title.as_deref(), Some("Short"));
        assert_eq!(ct.title_yomi.as_deref(), Some("Yomi"));
        assert_eq!(ct.title_en.as_deref(), Some("English"));
        assert_eq!(ct.cat, Some(1));
        assert_eq!(ct.title_flag, Some(0));
        assert_eq!(ct.first_year, Some(2024));
        assert_eq!(ct.first_month, Some(1));
        assert_eq!(ct.keywords, vec!["key1", "key2"]);
        assert_eq!(ct.sub_titles.as_deref(), Some("*01*EP1\r\n*02*EP2"));
        assert_eq!(ct.last_update, "2024-01-01T00:00:00Z");
        // TMDB fields must be None for fresh conversion
        assert!(ct.tmdb_series_id.is_none());
        assert!(ct.tmdb_season_number.is_none());
        assert!(ct.tmdb_season_id.is_none());
        assert!(ct.tmdb_original_name.is_none());
        assert!(ct.tmdb_name.is_none());
        assert!(ct.tmdb_alt_titles.is_none());
        assert!(ct.tmdb_last_updated.is_none());
    }

    #[test]
    fn test_to_cached_title_optional_fields_none() {
        // Arrange
        let src = SyoboiTitle {
            tid: 1,
            last_update: String::new(),
            title: "T".to_owned(),
            short_title: None,
            title_yomi: None,
            title_en: None,
            comment: None,
            cat: None,
            title_flag: None,
            first_year: None,
            first_month: None,
            first_end_year: None,
            first_end_month: None,
            first_ch: None,
            keywords: None,
            user_point: None,
            user_point_rank: None,
            sub_titles: None,
        };

        // Act
        let ct = to_cached_title(&src);

        // Assert
        assert!(ct.short_title.is_none());
        assert!(ct.title_yomi.is_none());
        assert!(ct.title_en.is_none());
        assert!(ct.cat.is_none());
        assert!(ct.title_flag.is_none());
        assert!(ct.first_year.is_none());
        assert!(ct.first_month.is_none());
        assert!(ct.keywords.is_empty());
        assert!(ct.sub_titles.is_none());
    }

    fn make_syoboi_program(pid: u32, tid: u32, ch_id: u32) -> SyoboiProgram {
        SyoboiProgram {
            pid,
            tid,
            st_time: "2024-01-15T20:00:00".to_owned(),
            st_offset: Some(-30),
            ed_time: "2024-01-15T20:30:00".to_owned(),
            count: Some(1),
            sub_title: Some("Episode 1".to_owned()),
            prog_comment: None,
            flag: Some(0),
            deleted: Some(0),
            warn: None,
            ch_id,
            revision: Some(1),
            last_update: Some("2024-01-15T00:00:00Z".to_owned()),
            st_sub_title: Some("Ep 1".to_owned()),
        }
    }

    #[test]
    fn test_to_cached_program_maps_all_fields() {
        // Arrange
        let src = make_syoboi_program(100, 42, 5);

        // Act
        let cp = to_cached_program(&src);

        // Assert
        assert_eq!(cp.pid, 100);
        assert_eq!(cp.tid, 42);
        assert_eq!(cp.ch_id, 5);
        assert_eq!(cp.st_time, "2024-01-15T20:00:00");
        assert_eq!(cp.st_offset, Some(-30));
        assert_eq!(cp.ed_time, "2024-01-15T20:30:00");
        assert_eq!(cp.count, Some(1));
        assert_eq!(cp.sub_title.as_deref(), Some("Episode 1"));
        assert_eq!(cp.flag, Some(0));
        assert_eq!(cp.deleted, Some(0));
        assert!(cp.warn.is_none());
        assert_eq!(cp.revision, Some(1));
        assert_eq!(cp.last_update.as_deref(), Some("2024-01-15T00:00:00Z"));
        assert_eq!(cp.st_sub_title.as_deref(), Some("Ep 1"));
        assert!(cp.tmdb_episode_id.is_none());
        assert!(cp.duration_min.is_none());
    }

    #[test]
    fn test_to_cached_program_optional_none() {
        // Arrange
        let src = SyoboiProgram {
            pid: 1,
            tid: 1,
            st_time: String::new(),
            st_offset: None,
            ed_time: String::new(),
            count: None,
            sub_title: None,
            prog_comment: None,
            flag: None,
            deleted: None,
            warn: None,
            ch_id: 1,
            revision: None,
            last_update: None,
            st_sub_title: None,
        };

        // Act
        let cp = to_cached_program(&src);

        // Assert
        assert!(cp.st_offset.is_none());
        assert!(cp.count.is_none());
        assert!(cp.sub_title.is_none());
        assert!(cp.flag.is_none());
        assert!(cp.deleted.is_none());
        assert!(cp.revision.is_none());
        assert!(cp.last_update.is_none());
        assert!(cp.st_sub_title.is_none());
    }

    // ── requires_animation_filter ──────────────────────────────

    #[test]
    fn test_requires_animation_filter_anime_cats() {
        // Act & Assert
        assert!(requires_animation_filter(Some(1)));
        assert!(requires_animation_filter(Some(7)));
        assert!(requires_animation_filter(Some(8)));
        assert!(requires_animation_filter(Some(10)));
    }

    #[test]
    fn test_requires_animation_filter_non_anime() {
        // Act & Assert
        assert!(!requires_animation_filter(None));
        assert!(!requires_animation_filter(Some(0)));
        assert!(!requires_animation_filter(Some(2)));
        assert!(!requires_animation_filter(Some(99)));
    }

    // ── resolve_media_type ─────────────────────────────────────

    #[test]
    fn test_resolve_media_type_movie() {
        // Arrange
        let cat_movie: HashSet<u32> = [4, 9].iter().copied().collect();

        // Act & Assert
        assert_eq!(
            resolve_media_type(Some(4), &cat_movie),
            TmdbMediaType::Movie
        );
        assert_eq!(
            resolve_media_type(Some(9), &cat_movie),
            TmdbMediaType::Movie
        );
    }

    #[test]
    fn test_resolve_media_type_tv_default() {
        // Arrange
        let cat_movie: HashSet<u32> = [4, 9].iter().copied().collect();

        // Act & Assert
        assert_eq!(resolve_media_type(None, &cat_movie), TmdbMediaType::Tv);
        assert_eq!(resolve_media_type(Some(1), &cat_movie), TmdbMediaType::Tv);
        assert_eq!(resolve_media_type(Some(99), &cat_movie), TmdbMediaType::Tv);
    }

    // ── extract_base_query ─────────────────────────────────────

    #[test]
    fn test_extract_base_query_no_regex() {
        // Act
        let result = extract_base_query("進撃の巨人", None);

        // Assert
        assert_eq!(result, "進撃の巨人");
    }

    #[test]
    fn test_extract_base_query_regex_removes_match() {
        // Arrange
        let re = regex::Regex::new(r"第\d+期$").unwrap();

        // Act
        let result = extract_base_query("進撃の巨人 第3期", Some(&re));

        // Assert
        assert_eq!(result, "進撃の巨人");
    }

    #[test]
    fn test_extract_base_query_regex_removes_all_returns_original() {
        // Arrange: regex matches entire normalized string
        let re = regex::Regex::new(r"^.+$").unwrap();

        // Act
        let result = extract_base_query("タイトル", Some(&re));

        // Assert: when result would be empty, return original normalized string
        assert_eq!(result, "タイトル");
    }

    // ── resolve_channel_name ───────────────────────────────────

    #[test]
    fn test_resolve_channel_name_from_arg() {
        // Act & Assert
        assert_eq!(resolve_channel_name(Some("NHK")), Some("NHK".to_owned()));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_resolve_channel_name_none() {
        // Arrange: clear env var to ensure None fallback
        unsafe { std::env::remove_var(CHANNEL_ENV_VAR) };

        // Act & Assert
        assert_eq!(resolve_channel_name(None), None);
    }

    // ── upsert_filtered_programs ───────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_upsert_filtered_programs_filters_by_tid_and_chid() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let conn = dtvmgr_db::open_db(Some(&dir.path().to_path_buf())).unwrap();

        // Insert required FK references
        dtvmgr_db::upsert_titles(
            &conn,
            &[CachedTitle {
                tid: 10,
                title: "T10".to_owned(),
                last_update: "2024-01-01".to_owned(),
                ..make_cached_title(10, None, None)
            }],
        )
        .unwrap();
        dtvmgr_db::upsert_channels(
            &conn,
            &[dtvmgr_db::channels::CachedChannel {
                ch_id: 20,
                ch_gid: None,
                ch_name: "CH20".to_owned(),
            }],
        )
        .unwrap();

        let programs = vec![
            make_syoboi_program(1, 10, 20), // valid
            make_syoboi_program(2, 10, 99), // invalid ch_id
            make_syoboi_program(3, 99, 20), // invalid tid
        ];
        let valid_tids: HashSet<u32> = [10].into();
        let valid_ch_ids: HashSet<u32> = [20].into();
        let all_fetched_tids: HashSet<u32> = [10, 99].into();

        // Act
        let (inserted, _) = upsert_filtered_programs(
            &conn,
            &programs,
            &valid_tids,
            &valid_ch_ids,
            &all_fetched_tids,
        )
        .unwrap();

        // Assert: only program with valid tid+ch_id passes
        assert_eq!(inserted, 1);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_upsert_filtered_programs_counts_cat_filtered_and_fk_missing() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let conn = dtvmgr_db::open_db(Some(&dir.path().to_path_buf())).unwrap();

        let programs = vec![
            make_syoboi_program(1, 50, 20), // tid 50 in all_fetched but not valid → cat filtered
            make_syoboi_program(2, 77, 20), // tid 77 NOT in all_fetched → fk missing
        ];
        let valid_tids: HashSet<u32> = HashSet::new();
        let valid_ch_ids: HashSet<u32> = [20].into();
        let all_fetched_tids: HashSet<u32> = [50].into();

        // Act
        let (inserted, _) = upsert_filtered_programs(
            &conn,
            &programs,
            &valid_tids,
            &valid_ch_ids,
            &all_fetched_tids,
        )
        .unwrap();

        // Assert
        assert_eq!(inserted, 0);
    }

    // ── cleanup_disallowed_cats ────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_cleanup_disallowed_cats_removes_and_cascades() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let conn = dtvmgr_db::open_db(Some(&dir.path().to_path_buf())).unwrap();

        // Insert titles with different cats
        let titles = vec![
            CachedTitle {
                tid: 1,
                cat: Some(1),
                title: "Anime".to_owned(),
                last_update: "2024-01-01".to_owned(),
                ..make_cached_title(1, None, None)
            },
            CachedTitle {
                tid: 2,
                cat: Some(99),
                title: "Other".to_owned(),
                last_update: "2024-01-01".to_owned(),
                ..make_cached_title(2, None, None)
            },
        ];
        dtvmgr_db::upsert_titles(&conn, &titles).unwrap();

        // Only allow cat=1
        let allowed: HashSet<u32> = [1].into();

        // Act
        cleanup_disallowed_cats(&conn, &allowed).unwrap();

        // Assert
        let remaining = dtvmgr_db::load_titles(&conn).unwrap();
        let tids: Vec<u32> = remaining.iter().map(|t| t.tid).collect();
        assert_eq!(tids, vec![1]);
    }

    // ── From<AvsTargetArg> ───────────────────────────────────

    #[test]
    fn test_from_avstargetarg_cutcm() {
        // Act
        let target: AvsTarget = AvsTargetArg::Cutcm.into();

        // Assert
        assert_eq!(target, AvsTarget::CutCm);
    }

    #[test]
    fn test_from_avstargetarg_cutcm_logo() {
        // Act
        let target: AvsTarget = AvsTargetArg::CutcmLogo.into();

        // Assert
        assert_eq!(target, AvsTarget::CutCmLogo);
    }

    // ── resolve_ch_ids ───────────────────────────────────────

    #[test]
    fn test_resolve_ch_ids_with_explicit_ids() {
        // Act
        let result = resolve_ch_ids(Some(vec![1, 2]), None);

        // Assert
        assert_eq!(result.unwrap(), vec![1, 2]);
    }

    // ── resolve_tmdb_language ────────────────────────────────

    #[test]
    fn test_resolve_tmdb_language_cli_arg() {
        // Act
        let lang = resolve_tmdb_language(Some("fr-FR"), None);

        // Assert
        assert_eq!(lang, "fr-FR");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_resolve_tmdb_language_fallback_default() {
        // Act: no CLI arg, nonexistent dir → config load fails → "en-US"
        let lang = resolve_tmdb_language(None, Some(&PathBuf::from("/nonexistent/path")));

        // Assert
        assert_eq!(lang, "en-US");
    }

    // ── build_tui_groups ─────────────────────────────────────

    #[test]
    fn test_build_tui_groups_maps_channels_to_groups() {
        // Arrange
        let groups = vec![
            CachedChannelGroup {
                ch_gid: 1,
                ch_group_name: String::from("Group A"),
                ch_group_order: 1,
            },
            CachedChannelGroup {
                ch_gid: 2,
                ch_group_name: String::from("Group B"),
                ch_group_order: 2,
            },
        ];
        let channels = vec![
            CachedChannel {
                ch_id: 10,
                ch_gid: Some(1),
                ch_name: String::from("Ch10"),
            },
            CachedChannel {
                ch_id: 20,
                ch_gid: Some(2),
                ch_name: String::from("Ch20"),
            },
            CachedChannel {
                ch_id: 11,
                ch_gid: Some(1),
                ch_name: String::from("Ch11"),
            },
        ];

        // Act
        let result = build_tui_groups(&groups, &channels);

        // Assert
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].ch_gid, 1);
        assert_eq!(result[0].channels.len(), 2);
        assert_eq!(result[0].channels[0].ch_id, 10);
        assert_eq!(result[0].channels[1].ch_id, 11);
        assert_eq!(result[1].ch_gid, 2);
        assert_eq!(result[1].channels.len(), 1);
        assert_eq!(result[1].channels[0].ch_id, 20);
    }

    #[test]
    fn test_build_tui_groups_sorts_by_order() {
        // Arrange: group B has lower order than group A
        let groups = vec![
            CachedChannelGroup {
                ch_gid: 1,
                ch_group_name: String::from("Group A"),
                ch_group_order: 5,
            },
            CachedChannelGroup {
                ch_gid: 2,
                ch_group_name: String::from("Group B"),
                ch_group_order: 1,
            },
        ];
        let channels: Vec<CachedChannel> = vec![];

        // Act
        let result = build_tui_groups(&groups, &channels);

        // Assert: Group B (order=1) should come first
        assert_eq!(result[0].ch_gid, 2);
        assert_eq!(result[1].ch_gid, 1);
    }

    #[test]
    fn test_build_tui_groups_skips_ungrouped() {
        // Arrange: channel with ch_gid = None
        let groups = vec![CachedChannelGroup {
            ch_gid: 1,
            ch_group_name: String::from("Group A"),
            ch_group_order: 1,
        }];
        let channels = vec![
            CachedChannel {
                ch_id: 10,
                ch_gid: Some(1),
                ch_name: String::from("Ch10"),
            },
            CachedChannel {
                ch_id: 99,
                ch_gid: None,
                ch_name: String::from("Ungrouped"),
            },
        ];

        // Act
        let result = build_tui_groups(&groups, &channels);

        // Assert: only grouped channel included
        assert_eq!(result[0].channels.len(), 1);
        assert_eq!(result[0].channels[0].ch_id, 10);
    }

    #[test]
    fn test_build_tui_groups_empty() {
        // Act
        let result = build_tui_groups(&[], &[]);

        // Assert
        assert!(result.is_empty());
    }
}
