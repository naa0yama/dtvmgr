//! dtvmgr - TV program data management CLI.

/// `OTel` metrics instruments for CLI commands.
#[cfg(feature = "otel")]
mod cli_metrics {
    use std::sync::LazyLock;

    use opentelemetry::metrics::{Counter, Meter};

    /// Shared meter for dtvmgr-cli.
    static METER: LazyLock<Meter> = LazyLock::new(|| opentelemetry::global::meter("dtvmgr-cli"));

    /// Records processed during DB sync.
    pub static DB_SYNC_RECORDS: LazyLock<Counter<u64>> = LazyLock::new(|| {
        METER
            .u64_counter("dtvmgr.db.sync.records")
            .with_description("Records processed during DB sync")
            .build()
    });

    /// TMDB lookup outcomes by type.
    pub static TMDB_LOOKUP_OUTCOMES: LazyLock<Counter<u64>> = LazyLock::new(|| {
        METER
            .u64_counter("dtvmgr.tmdb.lookup.outcomes")
            .with_description("TMDB lookup outcomes by type")
            .build()
    });
}

/// Application configuration (TOML).
mod config;
/// Terminal UI components.
mod tui;

use std::collections::{BTreeSet, HashSet};
use std::io::BufRead;
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{CommandFactory, Parser, Subcommand};
use tracing::{Instrument as _, instrument};
use tracing_subscriber::filter::EnvFilter;
#[cfg(not(feature = "otel"))]
use tracing_subscriber::fmt;
#[cfg(feature = "otel")]
use tracing_subscriber::layer::SubscriberExt;
#[cfg(feature = "otel")]
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::{AppConfig, load_or_fetch, resolve_config_path, resolve_data_dir};
use crate::tui::encode_selector::state::{
    EncodeQueueInfo, EncodeRow, EncodeSelectorState, FileCheckMessage, FileCheckRequest,
    FileCheckWorkerProgress, PageInfo, QueueMessage, RunningEncodeItem, SelectorResult,
    SyncMessage,
};
use crate::tui::run_channel_selector;
use crate::tui::state::{ChannelEntry, ChannelGroup};
use dtvmgr_api::epgstation::{
    EncodeRequest, EpgStationClient, LocalEpgStationApi, RecordedItem, RecordedParams,
    RecordedResponse,
};
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
use dtvmgr_jlse::progress::{ProgressEvent, ProgressMode};
use dtvmgr_jlse::settings::{BinaryPaths, DataPaths};
use dtvmgr_jlse::types::{AvsTarget, JlseConfig};

/// CLI argument parser.
#[derive(Parser)]
#[command(about, version)]
struct Cli {
    /// Path to config file (relative or absolute).
    /// Data directory defaults to the same directory as the config file.
    #[arg(long, global = true)]
    config: Option<PathBuf>,

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
    /// `EPGStation` operations.
    Epgstation(EpgstationCommand),
    /// Initialize config file with default template.
    Init,
    /// Generate shell completion script.
    Completion(CompletionCommand),
}

/// Arguments for the `epgstation` subcommand.
#[derive(clap::Args)]
struct EpgstationCommand {
    /// Epgstation subcommand to run.
    #[command(subcommand)]
    command: EpgstationSubcommands,
}

/// Available `EPGStation` subcommands.
#[derive(Subcommand)]
enum EpgstationSubcommands {
    /// Queue programs for encoding via `EPGStation`.
    Encode(EpgstationEncodeArgs),
}

/// Arguments for `epgstation encode`.
#[derive(clap::Args)]
struct EpgstationEncodeArgs {
    /// Keyword to pre-filter recordings.
    #[arg(short, long)]
    keyword: Option<String>,
    /// Number of recordings to fetch (default: 100).
    #[arg(short, long, default_value_t = 100)]
    limit: u64,
    /// Directly encode a specific recorded item (skip TUI).
    #[arg(long)]
    record_id: Option<u64>,
    /// Encode preset name (used with --record-id; falls back to config default).
    #[arg(short, long)]
    mode: Option<String>,
    /// Remove original file after encoding.
    #[arg(long, default_value_t = false)]
    remove_original: bool,
}

/// Arguments for the `completion` subcommand.
#[derive(clap::Args, Debug)]
struct CompletionCommand {
    /// Target shell.
    #[arg(value_enum)]
    shell: clap_complete::Shell,
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
    /// Enable EPGStation-compatible progress JSON output.
    /// Reads `INPUT` and `OUTPUT` from environment variables.
    #[arg(long)]
    epgstation: bool,
    /// Interactive TUI progress display.
    #[arg(long)]
    tui: bool,
    /// Skip pre-encode duration validation.
    #[arg(long)]
    skip_duration_check: bool,
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
#[instrument(skip_all, err(level = "error"))]
async fn run_syoboi_prog(args: &ProgArgs, config_file: Option<&PathBuf>) -> Result<()> {
    let client = SyoboiClient::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("failed to build API client")?;

    let range = resolve_time_range(args.time_since.as_deref(), args.time_until.as_deref())
        .context("failed to resolve time range")?;
    tracing::info!(
        "Time range: {} .. {}",
        range.start.format("%Y-%m-%d %H:%M:%S"),
        range.end.format("%Y-%m-%d %H:%M:%S"),
    );

    let ch_ids = resolve_ch_ids(args.ch_ids.clone(), config_file)
        .context("failed to resolve channel IDs")?;

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
#[instrument(skip_all, err(level = "error"))]
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
fn resolve_ch_ids(ch_ids: Option<Vec<u32>>, config_file: Option<&PathBuf>) -> Result<Vec<u32>> {
    if let Some(ids) = ch_ids {
        return Ok(ids);
    }

    let config_path = resolve_config_path(config_file).context("failed to resolve config path")?;
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
#[instrument(skip_all, err(level = "error"))]
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
#[instrument(skip_all, err(level = "error"))]
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
#[instrument(skip_all, err(level = "error"))]
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

#[instrument(skip_all, err(level = "error"))]
#[allow(clippy::too_many_lines)]
async fn run_db_sync(args: &DbSyncArgs, config_file: Option<&PathBuf>) -> Result<()> {
    let client = build_syoboi_client().context("failed to build Syoboi client")?;

    let config_path = resolve_config_path(config_file).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;
    let allowed_cats: HashSet<u32> = config.syoboi.titles.cat.iter().copied().collect();
    tracing::info!(?allowed_cats, "Category filter loaded from config");

    let range = resolve_time_range(args.time_since.as_deref(), args.time_until.as_deref())
        .context("failed to resolve time range")?;
    tracing::info!(
        "Time range: {} .. {}",
        range.start.format("%Y-%m-%d %H:%M:%S"),
        range.end.format("%Y-%m-%d %H:%M:%S"),
    );

    let ch_ids = resolve_ch_ids(args.ch_ids.clone(), config_file)
        .context("failed to resolve channel IDs")?;

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

    let all_titles = fetch_titles_chunked(&client, &unique_tids)
        .await
        .context("failed to fetch titles in chunks")?;
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
    let data_dir = resolve_data_dir(config_file).context("failed to resolve data directory")?;
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
    )
    .context("failed to upsert filtered programs")?;

    cleanup_disallowed_cats(&conn, &allowed_cats)
        .context("failed to clean up disallowed categories")?;

    tracing::info!(
        "Sync complete: {} titles ({} changed), {} programs ({} changed)",
        cached_titles.len(),
        titles_changed,
        total_programs,
        programs_changed,
    );

    #[cfg(feature = "otel")]
    {
        use opentelemetry::KeyValue;
        #[allow(clippy::as_conversions)]
        {
            cli_metrics::DB_SYNC_RECORDS.add(
                titles_changed as u64,
                &[
                    KeyValue::new("table", "titles"),
                    KeyValue::new("op", "upserted"),
                ],
            );
            cli_metrics::DB_SYNC_RECORDS.add(
                programs_changed as u64,
                &[
                    KeyValue::new("table", "programs"),
                    KeyValue::new("op", "upserted"),
                ],
            );
            cli_metrics::DB_SYNC_RECORDS.add(
                ch_changed as u64,
                &[
                    KeyValue::new("table", "channels"),
                    KeyValue::new("op", "upserted"),
                ],
            );
        }
    }

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
#[instrument(skip_all, err(level = "error"))]
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
#[instrument(skip_all, err(level = "error"))]
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
                        .await
                        .context("failed to fetch alternative titles for TV")?;
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
                        .await
                        .context("failed to fetch alternative titles for Movie")?;
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
#[instrument(skip_all)]
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
#[instrument(skip_all, err(level = "error"))]
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
    .await
    .context("TMDB search failed for base query")?
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
        .await
        .context("TMDB search failed for keyword fallback")?
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
#[instrument(skip_all, err(level = "error"))]
#[allow(clippy::too_many_lines)]
async fn run_db_tmdb_lookup(args: &DbTmdbLookupArgs, config_file: Option<&PathBuf>) -> Result<()> {
    let data_dir = resolve_data_dir(config_file).context("failed to resolve data directory")?;
    let conn = open_db(data_dir.as_ref()).context("failed to open database")?;
    let config_path = resolve_config_path(config_file).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;

    let language = resolve_tmdb_language(args.language.as_deref(), config_file);
    let tmdb_client = build_tmdb_client(config_file).context("failed to build TMDB client")?;

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

    #[cfg(feature = "otel")]
    {
        use opentelemetry::KeyValue;
        #[allow(clippy::as_conversions)]
        {
            cli_metrics::TMDB_LOOKUP_OUTCOMES
                .add(success_count as u64, &[KeyValue::new("outcome", "success")]);
            cli_metrics::TMDB_LOOKUP_OUTCOMES
                .add(skip_count as u64, &[KeyValue::new("outcome", "skipped")]);
            cli_metrics::TMDB_LOOKUP_OUTCOMES
                .add(error_count as u64, &[KeyValue::new("outcome", "error")]);
            cli_metrics::TMDB_LOOKUP_OUTCOMES
                .add(mapped_count as u64, &[KeyValue::new("outcome", "mapped")]);
        }
    }

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
#[instrument(skip_all, err(level = "error"))]
fn build_tmdb_client(config_file: Option<&PathBuf>) -> Result<TmdbClient> {
    let api_token = if let Ok(token) = std::env::var("TMDB_API_TOKEN") {
        token
    } else {
        let config_path =
            resolve_config_path(config_file).context("failed to resolve config path")?;
        let config = AppConfig::load(&config_path).context("failed to load config")?;
        config
            .tmdb
            .api_key
            .context("TMDB_API_TOKEN env var is not set and tmdb.api_key is not configured")?
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
fn resolve_tmdb_language(cli_lang: Option<&str>, config_file: Option<&PathBuf>) -> String {
    if let Some(lang) = cli_lang {
        return lang.to_owned();
    }
    if let Ok(config_path) = resolve_config_path(config_file)
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
#[instrument(skip_all, err(level = "error"))]
async fn run_tmdb_search_tv(args: &TmdbSearchTvArgs, config_file: Option<&PathBuf>) -> Result<()> {
    let client = build_tmdb_client(config_file)?;
    let language = resolve_tmdb_language(args.language.as_deref(), config_file);

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
#[instrument(skip_all, err(level = "error"))]
async fn run_tmdb_search_movie(
    args: &TmdbSearchMovieArgs,
    config_file: Option<&PathBuf>,
) -> Result<()> {
    let client = build_tmdb_client(config_file)?;
    let language = resolve_tmdb_language(args.language.as_deref(), config_file);

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
#[instrument(skip_all, err(level = "error"))]
async fn run_tmdb_tv_details(
    args: &TmdbTvDetailsArgs,
    config_file: Option<&PathBuf>,
) -> Result<()> {
    let client = build_tmdb_client(config_file)?;
    let language = resolve_tmdb_language(args.language.as_deref(), config_file);

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
#[instrument(skip_all, err(level = "error"))]
async fn run_tmdb_tv_season(args: &TmdbTvSeasonArgs, config_file: Option<&PathBuf>) -> Result<()> {
    let client = build_tmdb_client(config_file)?;
    let language = resolve_tmdb_language(args.language.as_deref(), config_file);

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
fn resolve_jlse_config(config_file: Option<&PathBuf>) -> Result<JlseConfig> {
    let config_path = resolve_config_path(config_file).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;
    let jlse = config
        .jlse
        .context("jlse config not found in dtvmgr.toml; add [jlse.dirs] with jl, logo, result")?;
    anyhow::ensure!(
        jlse.dirs.is_configured(),
        "jlse.dirs is not configured in dtvmgr.toml; set jl, logo, result paths"
    );
    Ok(jlse)
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
#[instrument(skip_all, err(level = "error"))]
fn run_jlse_tsduck(args: &JlseTsduckArgs, config_file: Option<&PathBuf>) -> Result<()> {
    let jlse_config = resolve_jlse_config(config_file)?;
    let bins = BinaryPaths::from_config(&jlse_config);

    // --sid takes priority over -c (channel name detection).
    let explicit_sid = if let Some(ref sid) = args.sid {
        Some(sid.clone())
    } else if args.channel.is_some() {
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
        let xml = dtvmgr_tsduck::command::extract_eit(&bins.tstables, &args.input)?;
        println!("=== EIT Program Information (SID: {sid}) ===");
        dtvmgr_tsduck::eit::parse_eit_xml_by_sid(&xml, sid)
            .with_context(|| format!("failed to parse EIT XML for SID {sid}"))?
    } else {
        // No explicit SID: extract PAT (PID 0) and EIT (PID 0x12) separately.
        let pat_xml = dtvmgr_tsduck::command::extract_pat(&bins.tstables, &args.input)?;
        let pat_sid = dtvmgr_tsduck::pat::parse_pat_first_service_id(&pat_xml)
            .context("failed to parse PAT XML")?;

        let eit_xml = dtvmgr_tsduck::command::extract_eit(&bins.tstables, &args.input)?;
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
    let recording_target = detect_target_from_middle(&bins.tstables, &args.input);
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
#[instrument(skip_all, err(level = "error"))]
fn run_jlse_channel(args: &JlseChannelArgs, config_file: Option<&PathBuf>) -> Result<()> {
    let jlse_config = resolve_jlse_config(config_file)?;
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
#[instrument(skip_all, err(level = "error"))]
fn run_jlse_param(args: &JlseParamArgs, config_file: Option<&PathBuf>) -> Result<()> {
    let jlse_config = resolve_jlse_config(config_file)?;
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

/// Spawns a pipeline thread and runs the TUI progress viewer.
///
/// Suppresses tracing output on the pipeline thread to prevent
/// log lines from corrupting the TUI alternate screen.
#[instrument(skip_all, err(level = "error"))]
fn run_pipeline_with_tui(ctx: PipelineContext) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::channel::<ProgressEvent>();
    let parent_span = tracing::Span::current();
    let handle = std::thread::spawn(move || {
        let _entered = parent_span.enter();
        let cb = move |event: ProgressEvent| {
            // Ignore send errors (receiver may have been dropped on quit)
            let _ = tx.send(event);
        };
        run_pipeline(&ctx, Some(&cb))
    });
    crate::tui::progress_viewer::run_progress_viewer(&rx, handle)
}

/// Runs the `jlse run` subcommand.
///
/// Executes the full CM detection pipeline.
///
/// # Errors
///
/// Returns an error if the pipeline fails.
#[instrument(skip_all, err(level = "error"))]
fn run_jlse_run(args: &JlseRunArgs, config_file: Option<&PathBuf>) -> Result<()> {
    let jlse_config = resolve_jlse_config(config_file)?;

    let channel_name = resolve_channel_name(args.channel.as_deref());

    if args.epgstation {
        // EPGStation mode: read INPUT/OUTPUT from environment variables.
        let input = args
            .input
            .clone()
            .or_else(|| std::env::var("INPUT").ok().map(PathBuf::from))
            .context("INPUT environment variable is required in --epgstation mode")?;

        let (out_dir, out_name, out_extension) = std::env::var("OUTPUT").map_or_else(
            |_| (args.outdir.clone(), args.outname.clone(), None),
            |output_str| {
                let output_path = PathBuf::from(&output_str);
                let dir = output_path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(Path::to_path_buf);
                let name = output_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned());
                let ext = output_path
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned());
                (dir, name, ext)
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
            out_extension,
            remove: args.remove,
            progress_mode: Some(ProgressMode::EpgStation),
            skip_duration_check: args.skip_duration_check,
        };

        if args.tui {
            run_pipeline_with_tui(ctx)
        } else {
            // EPGStation callback mode (JSON output)
            let cb = |event: ProgressEvent| {
                use dtvmgr_jlse::progress::emit_epgstation;
                match event {
                    ProgressEvent::StageStart { stage, total, name } => {
                        emit_epgstation(0.0, &format!("({stage}/{total}) {name}: starting"));
                    }
                    ProgressEvent::StageProgress { percent, log } => {
                        emit_epgstation(percent, &log);
                    }
                    ProgressEvent::Encoding { percent, log } => {
                        let prefixed = format!("(4/4) FFmpeg: {log}");
                        emit_epgstation(percent, &prefixed);
                    }
                    ProgressEvent::Log(_) | ProgressEvent::Finished => {}
                }
            };

            run_pipeline(&ctx, Some(&cb))
        }
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
            out_extension: None,
            remove: args.remove,
            progress_mode: None,
            skip_duration_check: args.skip_duration_check,
        };

        if args.tui {
            run_pipeline_with_tui(ctx)
        } else {
            run_pipeline(&ctx, None)
        }
    }
}

// ── Syoboi / TMDB helpers ────────────────────────────────────

/// Builds a `SyoboiClient` with default user agent.
///
/// # Errors
///
/// Returns an error if the client fails to build.
#[instrument(skip_all, err(level = "error"))]
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
#[instrument(skip_all, err(level = "error"))]
async fn run_channels_select(config_file: Option<&PathBuf>) -> Result<()> {
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
    let data_dir = resolve_data_dir(config_file).context("failed to resolve data directory")?;
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
    let config_path = resolve_config_path(config_file).context("failed to resolve config path")?;
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
#[instrument(skip_all, err(level = "error"))]
fn run_channels_list(config_file: Option<&PathBuf>) -> Result<()> {
    let config_path = resolve_config_path(config_file).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;

    if config.syoboi.channels.selected.is_empty() {
        tracing::info!("No channels selected. Run `syoboi channels select` to choose channels.");
        return Ok(());
    }

    // Try to load names from DB cache
    let data_dir = resolve_data_dir(config_file).ok().flatten();
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
#[instrument(skip_all, err(level = "error"))]
fn run_db_list(config_file: Option<&PathBuf>) -> Result<()> {
    let data_dir = resolve_data_dir(config_file).context("failed to resolve data directory")?;
    let conn = open_db(data_dir.as_ref()).context("failed to open database")?;
    let config_path = resolve_config_path(config_file).context("failed to resolve config path")?;
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
#[instrument(skip_all, err(level = "error"))]
fn run_db_normalize(config_file: Option<&PathBuf>) -> Result<()> {
    let data_dir = resolve_data_dir(config_file).context("failed to resolve data directory")?;
    let conn = open_db(data_dir.as_ref()).context("failed to open database")?;
    let config_path = resolve_config_path(config_file).context("failed to resolve config path")?;
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
#[allow(clippy::too_many_lines)]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Detect TUI mode to suppress fmt output (alternate screen conflicts).
    let tui_mode = match &cli.command {
        Commands::Epgstation(cmd) => match &cmd.command {
            EpgstationSubcommands::Encode(args) => args.record_id.is_none(),
        },
        Commands::Jlse(jlse) => match &jlse.command {
            JlseSubcommands::Run(args) => args.tui,
            _ => false,
        },
        _ => false,
    };

    #[cfg(not(feature = "otel"))]
    {
        if tui_mode {
            fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .with_writer(std::io::sink)
                .init();
        } else {
            fmt()
                .with_env_filter(
                    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
                )
                .with_target(false)
                .init();
        }
    }

    #[cfg(feature = "otel")]
    let (tracer_provider, logger_provider, meter_provider) = {
        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_writer(if tui_mode {
                tracing_subscriber::fmt::writer::BoxMakeWriter::new(std::io::sink)
            } else {
                tracing_subscriber::fmt::writer::BoxMakeWriter::new(std::io::stderr)
            });

        let otel_parts = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .filter(|ep| !ep.is_empty())
            .and_then(|_| {
                let span_exporter = opentelemetry_otlp::SpanExporter::builder()
                    .with_http()
                    .build()
                    .ok()?;

                let log_exporter = opentelemetry_otlp::LogExporter::builder()
                    .with_http()
                    .build()
                    .ok()?;

                let resource = opentelemetry_sdk::Resource::builder()
                    .with_service_name(env!("CARGO_PKG_NAME"))
                    .build();

                let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                    .with_resource(resource.clone())
                    .with_batch_exporter(span_exporter)
                    .build();

                let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
                    .with_http()
                    .build()
                    .ok()?;

                let meter_provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
                    .with_resource(resource.clone())
                    .with_reader(
                        opentelemetry_sdk::metrics::PeriodicReader::builder(metric_exporter)
                            .build(),
                    )
                    .build();

                opentelemetry::global::set_meter_provider(meter_provider.clone());

                let logger_provider = opentelemetry_sdk::logs::SdkLoggerProvider::builder()
                    .with_resource(resource)
                    .with_batch_exporter(log_exporter)
                    .build();

                let tracer = opentelemetry::trace::TracerProvider::tracer(
                    &tracer_provider,
                    env!("CARGO_PKG_NAME"),
                );

                let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
                let log_layer =
                    opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(
                        &logger_provider,
                    );

                Some((
                    otel_layer,
                    log_layer,
                    tracer_provider,
                    logger_provider,
                    meter_provider,
                ))
            });

        let (otel_layer, log_layer, tracer_provider, logger_provider, meter_provider) =
            match otel_parts {
                Some((o, l, tp, lp, mp)) => (Some(o), Some(l), Some(tp), Some(lp), Some(mp)),
                None => (None, None, None, None, None),
            };

        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(otel_layer)
            .with(log_layer)
            .init();

        (tracer_provider, logger_provider, meter_provider)
    };

    let result = match cli.command {
        Commands::Syoboi(cmd) => match cmd.command {
            SyoboiSubcommands::Prog(args) => run_syoboi_prog(&args, cli.config.as_ref()).await,
            SyoboiSubcommands::Titles(args) => run_syoboi_titles(&args).await,
            SyoboiSubcommands::Channels(ch) => match ch.command {
                ChannelsSubcommands::Select => run_channels_select(cli.config.as_ref()).await,
                ChannelsSubcommands::List => run_channels_list(cli.config.as_ref()),
            },
        },
        Commands::Tmdb(tmdb) => match tmdb.command {
            TmdbSubcommands::SearchTv(args) => run_tmdb_search_tv(&args, cli.config.as_ref()).await,
            TmdbSubcommands::SearchMovie(args) => {
                run_tmdb_search_movie(&args, cli.config.as_ref()).await
            }
            TmdbSubcommands::TvDetails(args) => {
                run_tmdb_tv_details(&args, cli.config.as_ref()).await
            }
            TmdbSubcommands::TvSeason(args) => run_tmdb_tv_season(&args, cli.config.as_ref()).await,
        },
        Commands::Db(db) => match db.command {
            DbSubcommands::Sync(args) => run_db_sync(&args, cli.config.as_ref()).await,
            DbSubcommands::List => run_db_list(cli.config.as_ref()),
            DbSubcommands::Normalize => run_db_normalize(cli.config.as_ref()),
            DbSubcommands::TmdbLookup(args) => run_db_tmdb_lookup(&args, cli.config.as_ref()).await,
        },
        Commands::Jlse(jlse) => match jlse.command {
            JlseSubcommands::Channel(args) => run_jlse_channel(&args, cli.config.as_ref()),
            JlseSubcommands::Param(args) => run_jlse_param(&args, cli.config.as_ref()),
            JlseSubcommands::Run(args) => run_jlse_run(&args, cli.config.as_ref()),
            JlseSubcommands::Tsduck(args) => run_jlse_tsduck(&args, cli.config.as_ref()),
        },
        Commands::Epgstation(cmd) => match cmd.command {
            EpgstationSubcommands::Encode(args) => {
                run_epgstation_encode(&args, cli.config.as_ref()).await
            }
        },
        Commands::Init => run_init(cli.config.as_ref()),
        Commands::Completion(comp) => {
            let mut cmd = Cli::command();
            clap_complete::generate(comp.shell, &mut cmd, "dtvmgr", &mut std::io::stdout());
            Ok(())
        }
    };

    #[cfg(feature = "otel")]
    {
        if let Some(provider) = logger_provider {
            provider
                .force_flush()
                .context("failed to flush OTel logger provider")?;
            provider
                .shutdown()
                .context("failed to shutdown OTel logger provider")?;
        }
        if let Some(provider) = meter_provider {
            provider
                .force_flush()
                .context("failed to flush OTel meter provider")?;
            provider
                .shutdown()
                .context("failed to shutdown OTel meter provider")?;
        }
        if let Some(provider) = tracer_provider {
            provider
                .force_flush()
                .context("failed to flush OTel tracer provider")?;
            provider
                .shutdown()
                .context("failed to shutdown OTel tracer provider")?;
        }
    }

    result
}

/// Converts an API `RecordedItem` page to cached DB items.
#[allow(clippy::cast_possible_wrap, clippy::as_conversions)]
fn convert_recorded_to_cached(
    records: &[RecordedItem],
    now: &str,
) -> (
    Vec<dtvmgr_db::recorded::CachedRecordedItem>,
    Vec<(i64, Vec<dtvmgr_db::recorded::CachedVideoFile>)>,
) {
    use dtvmgr_db::recorded::{CachedRecordedItem, CachedVideoFile};

    let items: Vec<CachedRecordedItem> = records
        .iter()
        .map(|rec| CachedRecordedItem {
            id: rec.id as i64,
            channel_id: rec.channel_id as i64,
            name: rec.name.clone(),
            description: rec.description.clone(),
            extended: rec.extended.clone(),
            start_at: rec.start_at as i64,
            end_at: rec.end_at as i64,
            is_recording: rec.is_recording,
            is_encoding: rec.is_encoding,
            is_protected: rec.is_protected,
            video_resolution: rec.video_resolution.clone(),
            video_type: rec.video_type.clone(),
            drop_cnt: rec.drop_log_file.as_ref().map_or(0, |d| d.drop_cnt as i64),
            error_cnt: rec.drop_log_file.as_ref().map_or(0, |d| d.error_cnt as i64),
            scrambling_cnt: rec
                .drop_log_file
                .as_ref()
                .map_or(0, |d| d.scrambling_cnt as i64),
            fetched_at: String::from(now),
        })
        .collect();

    let video_files: Vec<(i64, Vec<CachedVideoFile>)> = records
        .iter()
        .map(|rec| {
            let files: Vec<CachedVideoFile> = rec
                .video_files
                .iter()
                .map(|vf| CachedVideoFile {
                    id: vf.id as i64,
                    recorded_id: rec.id as i64,
                    name: vf.name.clone(),
                    filename: vf.filename.clone(),
                    file_type: vf.file_type.clone(),
                    size: vf.size as i64,
                    file_exists: None,
                    file_checked_at: None,
                })
                .collect();
            (rec.id as i64, files)
        })
        .collect();

    (items, video_files)
}

/// Processes a fetched API page: converts records to cached form, upserts to DB,
/// collects IDs, and reports progress. Returns updated total.
#[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
#[instrument(skip_all, err(level = "error"))]
fn process_fetched_page(
    conn: &dtvmgr_db::Connection,
    records: &[RecordedItem],
    now: &str,
    all_ids: &mut Vec<i64>,
    api_total: u64,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<()> {
    let (items, video_files) = convert_recorded_to_cached(records, now);
    for item in &items {
        all_ids.push(item.id);
    }
    dtvmgr_db::upsert_recorded_items(conn, &items, &video_files)
        .context("failed to upsert recorded items")?;
    let fetched = all_ids.len();
    #[allow(clippy::cast_possible_truncation)]
    let total = api_total as usize;
    on_progress(fetched, total);
    Ok(())
}

/// Fetches a single page of recorded items from the API.
#[instrument(skip_all, err(level = "error"))]
async fn fetch_recorded_page(
    client: &EpgStationClient,
    limit: u64,
    api_offset: u64,
    keyword: Option<&str>,
) -> Result<RecordedResponse> {
    let params = RecordedParams {
        has_original_file: Some(true),
        limit: Some(limit),
        offset: Some(api_offset),
        is_reverse: None,
        is_half_width: Some(true),
        keyword: keyword.map(String::from),
    };
    client
        .fetch_recorded(&params)
        .await
        .context("failed to fetch recorded programs")
}

/// Background sync: opens its own DB connection (required for `Send`).
#[instrument(skip_all, err(level = "error"))]
async fn sync_recorded_background(
    client: &EpgStationClient,
    data_dir: Option<&std::path::PathBuf>,
    limit: u64,
    keyword: Option<&str>,
    sync_tx: &std::sync::mpsc::Sender<SyncMessage>,
) -> Result<(Vec<i64>, dtvmgr_db::Connection)> {
    let conn = open_db(data_dir).context("failed to open database for sync")?;
    let now = chrono::Utc::now().to_rfc3339();
    let mut all_ids: Vec<i64> = Vec::new();
    let mut api_offset: u64 = 0;
    let mut api_total: u64 = 0;
    let tx = sync_tx.clone();
    let mut on_progress = move |fetched, total| {
        let _ = tx.send(SyncMessage::Progress { fetched, total });
    };

    loop {
        let recorded = fetch_recorded_page(client, limit, api_offset, keyword).await?;
        if api_total == 0 {
            api_total = recorded.total;
        }
        if recorded.records.is_empty() {
            break;
        }
        process_fetched_page(
            &conn,
            &recorded.records,
            &now,
            &mut all_ids,
            api_total,
            &mut on_progress,
        )?;
        api_offset = api_offset.saturating_add(limit);
        if api_offset >= api_total {
            break;
        }
    }
    Ok((all_ids, conn))
}

/// Blocking initial sync with terminal progress display.
#[allow(clippy::future_not_send)]
#[instrument(skip_all, err(level = "error"))]
async fn sync_recorded_initial(
    client: &EpgStationClient,
    conn: &dtvmgr_db::Connection,
    limit: u64,
    keyword: Option<&str>,
    terminal: &mut crate::tui::encode_selector::TuiTerminal,
) -> Result<Vec<i64>> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut all_ids: Vec<i64> = Vec::new();
    let mut api_offset: u64 = 0;
    let mut api_total: u64 = 0;

    loop {
        let recorded = fetch_recorded_page(client, limit, api_offset, keyword).await?;
        if api_total == 0 {
            api_total = recorded.total;
        }
        if recorded.records.is_empty() {
            break;
        }
        process_fetched_page(
            conn,
            &recorded.records,
            &now,
            &mut all_ids,
            api_total,
            &mut |fetched, total| {
                let _ =
                    crate::tui::encode_selector::draw_loading_progress(terminal, 0, fetched, total);
            },
        )?;
        api_offset = api_offset.saturating_add(limit);
        if api_offset >= api_total {
            break;
        }
    }
    Ok(all_ids)
}

/// Builds encode rows from cached recorded items.
#[allow(clippy::cast_sign_loss, clippy::as_conversions)]
fn build_rows_from_cache(
    cached: &[(
        dtvmgr_db::recorded::CachedRecordedItem,
        Vec<dtvmgr_db::recorded::CachedVideoFile>,
    )],
    channel_names: &std::collections::HashMap<u64, String>,
) -> Vec<EncodeRow> {
    cached
        .iter()
        .map(|(item, files)| {
            let ch_id = item.channel_id as u64;
            let ch_name = channel_names
                .get(&ch_id)
                .cloned()
                .unwrap_or_else(|| ch_id.to_string());

            let ts_file = files.iter().find(|vf| vf.file_type == "ts");

            let source_video_file_id = ts_file.map(|vf| vf.id as u64);
            let file_size = ts_file.map_or(0, |vf| vf.size as u64);
            let file_exists = ts_file.and_then(|vf| vf.file_exists).unwrap_or(false);

            EncodeRow {
                recorded_id: item.id as u64,
                channel_name: ch_name,
                name: item.name.clone(),
                start_at: item.start_at as u64,
                end_at: item.end_at as u64,
                video_resolution: item.video_resolution.clone().unwrap_or_default(),
                video_type: item.video_type.clone().unwrap_or_default(),
                source_video_file_id,
                file_size,
                drop_cnt: item.drop_cnt as u64,
                error_cnt: item.error_cnt as u64,
                is_recording: item.is_recording,
                is_encoding: item.is_encoding,
                file_exists,
            }
        })
        .collect()
}

/// Collects TS video files that need existence checking, using TTL cache logic.
/// Returns a list of `(video_file_id, recorded_id)` pairs (as `i64`).
fn collect_files_to_check(
    cached: &[(
        dtvmgr_db::recorded::CachedRecordedItem,
        Vec<dtvmgr_db::recorded::CachedVideoFile>,
    )],
    force: bool,
) -> Vec<(i64, i64)> {
    let now = chrono::Utc::now();
    let ttl_secs: i64 = 3600; // 1 hour

    let mut to_check: Vec<(i64, i64)> = Vec::new();
    for (item, files) in cached {
        for vf in files {
            if vf.file_type != "ts" {
                continue;
            }
            if force {
                to_check.push((vf.id, item.id));
                continue;
            }
            let needs_check = match (&vf.file_exists, &vf.file_checked_at) {
                (Some(_), Some(checked_at)) => chrono::DateTime::parse_from_rfc3339(checked_at)
                    .map_or(true, |checked| {
                        now.signed_duration_since(checked).num_seconds() > ttl_secs
                    }),
                _ => true,
            };
            if needs_check {
                to_check.push((vf.id, item.id));
            }
        }
    }
    to_check
}

/// Spawns a single long-lived worker that processes file existence check requests sequentially.
///
/// Each request contains a batch of files to check and a result channel for the requesting page.
/// Only one API call is ever in-flight at a time. Old page results silently fail to send when
/// the receiver is dropped, but DB updates still complete (useful for caching).
fn spawn_file_check_worker(
    client: &EpgStationClient,
    data_dir: Option<&PathBuf>,
    mut req_rx: tokio::sync::mpsc::Receiver<FileCheckRequest>,
    progress_tx: tokio::sync::watch::Sender<FileCheckWorkerProgress>,
    pending: Arc<AtomicUsize>,
) {
    let bg_client = client.clone();
    let bg_data_dir = data_dir.cloned();
    tokio::spawn(
        async move {
            let Ok(conn) = open_db(bg_data_dir.as_ref()) else {
                return;
            };
            while let Some(req) = req_rx.recv().await {
                let p = pending.fetch_sub(1, Ordering::Relaxed).saturating_sub(1);
                let total = req.files.len();
                let _ = progress_tx.send(FileCheckWorkerProgress {
                    pending: p,
                    checking: Some((0, total)),
                });
                let now_str = chrono::Utc::now().to_rfc3339();
                for (i, (vf_id, recorded_id)) in req.files.iter().enumerate() {
                    #[allow(clippy::cast_sign_loss, clippy::as_conversions)]
                    let exists = bg_client.check_video_file_exists(*vf_id as u64).await;
                    let _ = dtvmgr_db::update_file_exists(&conn, *vf_id, exists, &now_str);
                    #[allow(clippy::cast_sign_loss, clippy::as_conversions)]
                    let _ = req.result_tx.send(FileCheckMessage::Result {
                        recorded_id: *recorded_id as u64,
                        exists,
                    });
                    let _ = progress_tx.send(FileCheckWorkerProgress {
                        pending: pending.load(Ordering::Relaxed),
                        checking: Some((i.saturating_add(1), total)),
                    });
                }
                let _ = req.result_tx.send(FileCheckMessage::Complete);
                let _ = progress_tx.send(FileCheckWorkerProgress {
                    pending: pending.load(Ordering::Relaxed),
                    checking: None,
                });
            }
        }
        .instrument({
            let span = tracing::info_span!(parent: tracing::Span::none(), "file_check_worker");
            span.follows_from(tracing::Span::current());
            span
        }),
    );
}

/// Direct encode mode: fetch a single recorded item and submit an encode job.
#[allow(clippy::print_stdout)]
#[instrument(skip_all, err(level = "error"))]
async fn run_epgstation_encode_direct(
    client: &EpgStationClient,
    config: &AppConfig,
    record_id: u64,
    args: &EpgstationEncodeArgs,
) -> Result<()> {
    let recorded = client
        .fetch_recorded_by_id(record_id)
        .await
        .with_context(|| format!("failed to fetch recorded item {record_id}"))?;

    let source = recorded
        .video_files
        .iter()
        .find(|vf| vf.file_type == "ts")
        .context("no ts video file found for this recorded item")?;

    let mode = args
        .mode
        .clone()
        .or_else(|| config.epgstation.default_preset.clone())
        .context("--mode is required (no default_preset in config)")?;

    let parent_dir = config.epgstation.default_directory.clone();
    let is_save_same_directory = parent_dir.is_none();

    let request = EncodeRequest {
        recorded_id: record_id,
        source_video_file_id: source.id,
        mode,
        parent_dir,
        directory: None,
        is_save_same_directory,
        remove_original: args.remove_original,
    };

    let request_json =
        serde_json::to_string_pretty(&request).context("failed to serialize encode request")?;
    println!("Request:\n{request_json}");

    match client.add_encode(&request).await {
        Ok(resp) => {
            println!("Success: encode_id={}", resp.encode_id);
        }
        Err(e) => {
            println!("Error: {e:#}");
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    clippy::print_stdout,
    clippy::future_not_send,
    clippy::as_conversions,
    clippy::cast_possible_wrap
)]
#[instrument(skip_all, err(level = "error"))]
async fn run_epgstation_encode(
    args: &EpgstationEncodeArgs,
    config_file: Option<&PathBuf>,
) -> Result<()> {
    let config_path = resolve_config_path(config_file).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;
    let base_url = config
        .epgstation
        .base_url
        .as_deref()
        .unwrap_or("http://localhost:8888");

    let api_base = format!("{base_url}/api/");

    let client = EpgStationClient::builder()
        .base_url(api_base.parse().context("invalid EPGStation base URL")?)
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("failed to build EPGStation client")?;

    // Direct mode: skip TUI and submit a single encode job.
    if let Some(record_id) = args.record_id {
        return run_epgstation_encode_direct(&client, &config, record_id, args).await;
    }

    // Fetch channels and config once (shared across pages)
    let (channels_result, config_result) =
        tokio::join!(client.fetch_channels(), client.fetch_config());

    let channels = channels_result.context("failed to fetch channels")?;
    let epg_config = config_result.context("failed to fetch EPGStation config")?;

    let channel_names: std::collections::HashMap<u64, String> = channels
        .iter()
        .map(|ch| (ch.id, ch.half_width_name.clone()))
        .collect();

    let presets: Vec<String> = epg_config.encode.iter().map(|p| p.name.clone()).collect();
    let parent_dirs: Vec<String> = epg_config.recorded.iter().map(|d| d.name.clone()).collect();

    let limit = args.limit;
    let mut offset: u64 = 0;
    let mut selected = std::collections::BTreeSet::<u64>::new();
    let mut last_encode_queue: Option<EncodeQueueInfo> = None;

    // Open DB for caching
    let data_dir = resolve_data_dir(config_file).context("failed to resolve data directory")?;
    let conn = open_db(data_dir.as_ref()).context("failed to open database")?;

    // Set up terminal
    let mut terminal =
        crate::tui::encode_selector::setup_terminal().context("failed to set up terminal")?;

    // Check if DB has cached data
    #[allow(clippy::cast_possible_wrap)]
    let (cached_first_page, _cached_total) =
        dtvmgr_db::load_recorded_items_page(&conn, 0, limit as i64)
            .context("failed to load cached recorded items")?;
    let has_cache = !cached_first_page.is_empty();

    // Sync receiver for background updates
    let (sync_tx, sync_rx) = std::sync::mpsc::channel();
    let sync_rx_opt = if has_cache {
        // Has cache: spawn background re-sync
        let bg_client = client.clone();
        let bg_data_dir = data_dir.clone();
        let bg_keyword = args.keyword.clone();
        let bg_limit = limit;
        tokio::spawn(
            async move {
                let Ok((all_ids, bg_conn)) = sync_recorded_background(
                    &bg_client,
                    bg_data_dir.as_ref(),
                    bg_limit,
                    bg_keyword.as_deref(),
                    &sync_tx,
                )
                .await
                else {
                    return;
                };
                if !all_ids.is_empty() {
                    let _ = dtvmgr_db::delete_recorded_items_not_in(&bg_conn, &all_ids);
                }
                let _ = sync_tx.send(SyncMessage::Complete);
            }
            .instrument({
                let span =
                    tracing::info_span!(parent: tracing::Span::none(), "recorded_sync_worker");
                span.follows_from(tracing::Span::current());
                span
            }),
        );
        Some(sync_rx)
    } else {
        // No cache: do a blocking initial sync with progress
        let _ = crate::tui::encode_selector::draw_loading_progress(&mut terminal, 0, 0, 0);
        let all_ids = match sync_recorded_initial(
            &client,
            &conn,
            limit,
            args.keyword.as_deref(),
            &mut terminal,
        )
        .await
        {
            Ok(ids) => ids,
            Err(e) => {
                let _ = crate::tui::encode_selector::teardown_terminal();
                return Err(e).context("failed to sync recorded programs");
            }
        };
        // Clean up stale entries
        if !all_ids.is_empty() {
            let _ = dtvmgr_db::delete_recorded_items_not_in(&conn, &all_ids);
        }
        drop(sync_tx);
        None
    };

    // Spawn encode queue polling task (shared across all pages).
    let (queue_tx, queue_rx) = std::sync::mpsc::channel::<QueueMessage>();
    {
        let poll_client = client.clone();
        tokio::spawn(
            async move {
                let mut last_info: Option<EncodeQueueInfo> = None;
                loop {
                    if let Ok(resp) = poll_client.fetch_encode_queue().await {
                        let info = EncodeQueueInfo {
                            running: resp
                                .running_items
                                .iter()
                                .map(|item| RunningEncodeItem {
                                    name: item.recorded.name.clone(),
                                    mode: item.mode.clone(),
                                    percent: item.percent,
                                })
                                .collect(),
                            waiting_count: resp.wait_items.len(),
                        };
                        // Only send if changed from last update.
                        if last_info.as_ref() != Some(&info) {
                            last_info = Some(info.clone());
                            if queue_tx.send(QueueMessage::Update(info)).is_err() {
                                break;
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
            .instrument({
                let span =
                    tracing::info_span!(parent: tracing::Span::none(), "encode_queue_poller");
                span.follows_from(tracing::Span::current());
                span
            }),
        );
    }

    // Spawn a single file-check worker; page loop sends requests through this channel.
    let (check_req_tx, check_req_rx) = tokio::sync::mpsc::channel::<FileCheckRequest>(4);
    let pending = Arc::new(AtomicUsize::new(0));
    let (progress_tx, progress_rx) =
        tokio::sync::watch::channel(FileCheckWorkerProgress::default());
    spawn_file_check_worker(
        &client,
        data_dir.as_ref(),
        check_req_rx,
        progress_tx,
        Arc::clone(&pending),
    );

    loop {
        terminal.clear().context("failed to clear terminal")?;

        // Load current page from DB cache
        #[allow(clippy::cast_possible_wrap)]
        let (cached_page, total_items) =
            match dtvmgr_db::load_recorded_items_page(&conn, offset as i64, limit as i64) {
                Ok(r) => r,
                Err(e) => {
                    let _ = crate::tui::encode_selector::teardown_terminal();
                    return Err(e).context("failed to load cached recorded items");
                }
            };

        let rows = build_rows_from_cache(&cached_page, &channel_names);

        if rows.is_empty() && selected.is_empty() {
            crate::tui::encode_selector::teardown_terminal()
                .context("failed to tear down terminal")?;
            println!("No recorded programs found.");
            return Ok(());
        }

        let page = PageInfo {
            offset,
            size: limit,
            total: total_items,
        };
        let mut state = EncodeSelectorState::new(
            rows,
            presets.clone(),
            parent_dirs.clone(),
            config.epgstation.default_preset.as_deref(),
            config.epgstation.default_directory.as_deref(),
            page,
        );
        // Carry over selections and encode queue across pages (move, not clone)
        state.selected = mem::take(&mut selected);
        state.encode_queue = last_encode_queue.take();

        // Inner loop: on Refresh, keep state and enqueue a new file check request.
        // Old page's result_tx.send() returns Err (rx dropped) and is ignored; DB writes
        // still complete so no work is wasted.
        let mut is_force = false;
        let result = loop {
            let files_to_check = collect_files_to_check(&cached_page, is_force);
            let total_checks = files_to_check.len();
            let file_check_rx = if total_checks > 0 {
                let (file_check_tx, rx) = std::sync::mpsc::channel::<FileCheckMessage>();
                pending.fetch_add(1, Ordering::Relaxed);
                let _ = check_req_tx
                    .send(FileCheckRequest {
                        files: files_to_check,
                        result_tx: file_check_tx,
                    })
                    .await;
                Some(rx)
            } else {
                None
            };

            let r = match crate::tui::encode_selector::run_encode_selector(
                &mut terminal,
                &mut state,
                sync_rx_opt.as_ref(),
                file_check_rx.as_ref(),
                Some(&queue_rx),
                &progress_rx,
            )
            .await
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = crate::tui::encode_selector::teardown_terminal();
                    return Err(e).context("encode selector TUI failed");
                }
            };

            if r == SelectorResult::Refresh {
                is_force = true;
                continue;
            }
            break r;
        };

        // Preserve selections and encode queue before potentially continuing (move back)
        selected = mem::take(&mut state.selected);
        last_encode_queue = state.encode_queue.take();

        match result {
            SelectorResult::Confirmed => {
                // Submit encode jobs, then return to the selection screen.
                let confirmed_rows: Vec<_> = state
                    .rows
                    .iter()
                    .filter(|row| selected.contains(&row.recorded_id))
                    .collect();

                for row in &confirmed_rows {
                    let Some(source_id) = row.source_video_file_id else {
                        tracing::warn!(
                            recorded_id = row.recorded_id,
                            "skipping: no source video file"
                        );
                        continue;
                    };

                    let request = EncodeRequest {
                        recorded_id: row.recorded_id,
                        source_video_file_id: source_id,
                        mode: state.settings.mode.clone(),
                        parent_dir: if state.settings.is_save_same_directory {
                            None
                        } else {
                            Some(state.settings.parent_dir.clone())
                        },
                        directory: if state.settings.directory.is_empty()
                            || state.settings.is_save_same_directory
                        {
                            None
                        } else {
                            Some(state.settings.directory.clone())
                        },
                        is_save_same_directory: state.settings.is_save_same_directory,
                        remove_original: state.settings.remove_original,
                    };

                    match client.add_encode(&request).await {
                        Ok(resp) => {
                            tracing::info!(
                                recorded_id = row.recorded_id,
                                encode_id = resp.encode_id,
                                name = %row.name,
                                "encode job queued"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                recorded_id = row.recorded_id,
                                name = %row.name,
                                error = %e,
                                "failed to queue encode job"
                            );
                        }
                    }
                }

                // Clear selections and continue to the recording list.
                selected.clear();
            }
            SelectorResult::Cancelled => {
                crate::tui::encode_selector::teardown_terminal()
                    .context("failed to tear down terminal")?;
                return Ok(());
            }
            SelectorResult::PageNext => {
                offset = offset.saturating_add(limit);
            }
            SelectorResult::PagePrev => {
                offset = offset.saturating_sub(limit);
            }
            // Refresh is fully handled by the inner loop above.
            SelectorResult::Refresh => {}
        }
    }
}

/// Initialize config file with default template.
#[allow(clippy::print_stdout)]
fn run_init(config_file: Option<&PathBuf>) -> Result<()> {
    let path = resolve_config_path(config_file).context("failed to resolve config path")?;
    let default_config = AppConfig::default();
    let template = default_config.to_commented_toml();

    match std::fs::read_to_string(&path) {
        Ok(existing) if existing == template => {
            println!("Config already up to date: {}", path.display());
        }
        Ok(_) => {
            println!("Template (would be written to {}):\n", path.display());
            print!("{template}");
            print!("Overwrite existing config? [y/N] ");
            std::io::Write::flush(&mut std::io::stdout()).context("failed to flush stdout")?;
            let mut answer = String::new();
            std::io::stdin()
                .lock()
                .read_line(&mut answer)
                .context("failed to read from stdin")?;
            if answer.trim().eq_ignore_ascii_case("y") {
                AppConfig::write_toml(&path, &template)
                    .with_context(|| format!("failed to write {}", path.display()))?;
                println!("Config overwritten: {}", path.display());
            } else {
                println!("Aborted.");
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            AppConfig::write_toml(&path, &template)
                .with_context(|| format!("failed to create config at {}", path.display()))?;
            println!("Config created: {}", path.display());
        }
        Err(e) => {
            return Err(e).with_context(|| format!("failed to read {}", path.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::undocumented_unsafe_blocks,
        clippy::indexing_slicing
    )]

    use super::*;
    use dtvmgr_api::epgstation::{DropLogFile, VideoFile};

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
        // Act: no CLI arg, nonexistent dir → template parse gives "ja-JP"
        let lang = resolve_tmdb_language(None, Some(&PathBuf::from("/nonexistent/path")));

        // Assert
        assert_eq!(lang, "ja-JP");
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

    // ── extract_base_query (additional) ──────────────────────────

    #[test]
    fn test_extract_base_query_empty_string() {
        // Act
        let result = extract_base_query("", None);

        // Assert
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_base_query_regex_partial_middle() {
        // Arrange: regex matches in the middle of the string
        let re = regex::Regex::new(r"\s*第\d+期\s*").unwrap();

        // Act
        let result = extract_base_query("進撃の巨人 第3期 完結編", Some(&re));

        // Assert: middle portion removed, rest joined
        assert_eq!(result, "進撃の巨人完結編");
    }

    #[test]
    fn test_extract_base_query_regex_no_match() {
        // Arrange: regex doesn't match the title
        let re = regex::Regex::new(r"Season\s*\d+").unwrap();

        // Act
        let result = extract_base_query("進撃の巨人", Some(&re));

        // Assert: normalized string returned unchanged
        assert_eq!(result, "進撃の巨人");
    }

    // ── convert_recorded_to_cached ───────────────────────────────

    #[allow(clippy::arithmetic_side_effects)]
    fn make_recorded_item(id: u64) -> RecordedItem {
        RecordedItem {
            id,
            channel_id: 100,
            name: format!("Program {id}"),
            description: Some(String::from("desc")),
            extended: Some(String::from("ext")),
            start_at: 1_700_000_000_000,
            end_at: 1_700_001_800_000,
            is_recording: false,
            is_encoding: false,
            is_protected: false,
            video_resolution: Some(String::from("1080i")),
            video_type: Some(String::from("mpeg2")),
            video_files: vec![VideoFile {
                id: id * 10,
                name: format!("file_{id}.ts"),
                filename: Some(format!("file_{id}.ts")),
                file_type: String::from("ts"),
                size: 1_048_576,
            }],
            drop_log_file: Some(DropLogFile {
                drop_cnt: 5,
                error_cnt: 2,
                scrambling_cnt: 1,
            }),
        }
    }

    #[test]
    fn test_convert_recorded_to_cached_item_fields() {
        // Arrange
        let records = vec![make_recorded_item(1)];
        let now = "2024-01-01T00:00:00Z";

        // Act
        let (items, _) = convert_recorded_to_cached(&records, now);

        // Assert
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, 1);
        assert_eq!(items[0].channel_id, 100);
        assert_eq!(items[0].name, "Program 1");
        assert_eq!(items[0].description.as_deref(), Some("desc"));
        assert_eq!(items[0].extended.as_deref(), Some("ext"));
        assert_eq!(items[0].start_at, 1_700_000_000_000);
        assert_eq!(items[0].end_at, 1_700_001_800_000);
        assert!(!items[0].is_recording);
        assert!(!items[0].is_encoding);
        assert!(!items[0].is_protected);
        assert_eq!(items[0].video_resolution.as_deref(), Some("1080i"));
        assert_eq!(items[0].video_type.as_deref(), Some("mpeg2"));
        assert_eq!(items[0].fetched_at, now);
    }

    #[test]
    fn test_convert_recorded_to_cached_drop_log_fields() {
        // Arrange
        let records = vec![make_recorded_item(1)];

        // Act
        let (items, _) = convert_recorded_to_cached(&records, "2024-01-01T00:00:00Z");

        // Assert
        assert_eq!(items[0].drop_cnt, 5);
        assert_eq!(items[0].error_cnt, 2);
        assert_eq!(items[0].scrambling_cnt, 1);
    }

    #[test]
    fn test_convert_recorded_to_cached_video_files() {
        // Arrange
        let records = vec![make_recorded_item(1)];

        // Act
        let (_, video_files) = convert_recorded_to_cached(&records, "2024-01-01T00:00:00Z");

        // Assert
        assert_eq!(video_files.len(), 1);
        assert_eq!(video_files[0].0, 1); // recorded_id
        assert_eq!(video_files[0].1.len(), 1);
        assert_eq!(video_files[0].1[0].id, 10);
        assert_eq!(video_files[0].1[0].file_type, "ts");
        assert_eq!(video_files[0].1[0].size, 1_048_576);
        assert!(video_files[0].1[0].file_exists.is_none());
        assert!(video_files[0].1[0].file_checked_at.is_none());
    }

    #[test]
    fn test_convert_recorded_to_cached_empty_input() {
        // Act
        let (items, video_files) = convert_recorded_to_cached(&[], "2024-01-01T00:00:00Z");

        // Assert
        assert!(items.is_empty());
        assert!(video_files.is_empty());
    }

    #[test]
    fn test_convert_recorded_to_cached_multiple_records() {
        // Arrange
        let records = vec![make_recorded_item(1), make_recorded_item(2)];

        // Act
        let (items, video_files) = convert_recorded_to_cached(&records, "2024-01-01T00:00:00Z");

        // Assert
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, 1);
        assert_eq!(items[1].id, 2);
        assert_eq!(video_files.len(), 2);
        assert_eq!(video_files[0].0, 1);
        assert_eq!(video_files[1].0, 2);
    }

    #[test]
    fn test_convert_recorded_to_cached_empty_video_files() {
        // Arrange
        let mut rec = make_recorded_item(1);
        rec.video_files = vec![];

        // Act
        let (items, video_files) = convert_recorded_to_cached(&[rec], "2024-01-01T00:00:00Z");

        // Assert
        assert_eq!(items.len(), 1);
        assert_eq!(video_files.len(), 1);
        assert!(video_files[0].1.is_empty());
    }

    #[test]
    fn test_convert_recorded_to_cached_no_drop_log() {
        // Arrange
        let mut rec = make_recorded_item(1);
        rec.drop_log_file = None;

        // Act
        let (items, _) = convert_recorded_to_cached(&[rec], "2024-01-01T00:00:00Z");

        // Assert: drop/error/scrambling default to 0
        assert_eq!(items[0].drop_cnt, 0);
        assert_eq!(items[0].error_cnt, 0);
        assert_eq!(items[0].scrambling_cnt, 0);
    }

    #[test]
    fn test_convert_recorded_to_cached_optional_fields_none() {
        // Arrange
        let mut rec = make_recorded_item(1);
        rec.description = None;
        rec.extended = None;
        rec.video_resolution = None;
        rec.video_type = None;

        // Act
        let (items, _) = convert_recorded_to_cached(&[rec], "2024-01-01T00:00:00Z");

        // Assert
        assert!(items[0].description.is_none());
        assert!(items[0].extended.is_none());
        assert!(items[0].video_resolution.is_none());
        assert!(items[0].video_type.is_none());
    }

    #[test]
    fn test_convert_recorded_to_cached_multiple_video_files() {
        // Arrange
        let mut rec = make_recorded_item(1);
        rec.video_files = vec![
            VideoFile {
                id: 10,
                name: String::from("ts_file"),
                filename: Some(String::from("ts_file.ts")),
                file_type: String::from("ts"),
                size: 2_000_000,
            },
            VideoFile {
                id: 11,
                name: String::from("encoded_file"),
                filename: Some(String::from("encoded.mp4")),
                file_type: String::from("encoded"),
                size: 500_000,
            },
        ];

        // Act
        let (_, video_files) = convert_recorded_to_cached(&[rec], "2024-01-01T00:00:00Z");

        // Assert
        assert_eq!(video_files[0].1.len(), 2);
        assert_eq!(video_files[0].1[0].file_type, "ts");
        assert_eq!(video_files[0].1[1].file_type, "encoded");
    }

    // ── build_rows_from_cache ────────────────────────────────────

    fn make_cached_pair(
        id: i64,
        channel_id: i64,
        files: Vec<dtvmgr_db::recorded::CachedVideoFile>,
    ) -> (
        dtvmgr_db::recorded::CachedRecordedItem,
        Vec<dtvmgr_db::recorded::CachedVideoFile>,
    ) {
        (
            dtvmgr_db::recorded::CachedRecordedItem {
                id,
                channel_id,
                name: format!("Program {id}"),
                description: None,
                extended: None,
                start_at: 1_700_000_000_000,
                end_at: 1_700_001_800_000,
                is_recording: false,
                is_encoding: false,
                is_protected: false,
                video_resolution: Some(String::from("1080i")),
                video_type: Some(String::from("mpeg2")),
                drop_cnt: 3,
                error_cnt: 1,
                scrambling_cnt: 0,
                fetched_at: String::from("2024-01-01T00:00:00Z"),
            },
            files,
        )
    }

    fn make_ts_video_file(id: i64, recorded_id: i64) -> dtvmgr_db::recorded::CachedVideoFile {
        dtvmgr_db::recorded::CachedVideoFile {
            id,
            recorded_id,
            name: String::from("ts_file"),
            filename: Some(String::from("file.ts")),
            file_type: String::from("ts"),
            size: 2_000_000,
            file_exists: Some(true),
            file_checked_at: Some(String::from("2024-01-01T00:00:00Z")),
        }
    }

    #[test]
    fn test_build_rows_from_cache_basic() {
        // Arrange
        let cached = vec![make_cached_pair(1, 100, vec![make_ts_video_file(10, 1)])];
        let mut channel_names = std::collections::HashMap::new();
        channel_names.insert(100_u64, String::from("NHK"));

        // Act
        let rows = build_rows_from_cache(&cached, &channel_names);

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].recorded_id, 1);
        assert_eq!(rows[0].channel_name, "NHK");
        assert_eq!(rows[0].name, "Program 1");
        assert_eq!(rows[0].source_video_file_id, Some(10));
        assert_eq!(rows[0].file_size, 2_000_000);
        assert!(rows[0].file_exists);
        assert_eq!(rows[0].drop_cnt, 3);
        assert_eq!(rows[0].error_cnt, 1);
        assert!(!rows[0].is_recording);
        assert!(!rows[0].is_encoding);
    }

    #[test]
    fn test_build_rows_from_cache_unknown_channel() {
        // Arrange: channel_id not in channel_names map
        let cached = vec![make_cached_pair(1, 999, vec![make_ts_video_file(10, 1)])];
        let channel_names = std::collections::HashMap::new();

        // Act
        let rows = build_rows_from_cache(&cached, &channel_names);

        // Assert: falls back to channel_id as string
        assert_eq!(rows[0].channel_name, "999");
    }

    #[test]
    fn test_build_rows_from_cache_no_ts_file() {
        // Arrange: only encoded file, no TS
        let encoded_file = dtvmgr_db::recorded::CachedVideoFile {
            id: 20,
            recorded_id: 1,
            name: String::from("encoded"),
            filename: Some(String::from("encoded.mp4")),
            file_type: String::from("encoded"),
            size: 500_000,
            file_exists: Some(true),
            file_checked_at: None,
        };
        let cached = vec![make_cached_pair(1, 100, vec![encoded_file])];
        let mut channel_names = std::collections::HashMap::new();
        channel_names.insert(100_u64, String::from("NHK"));

        // Act
        let rows = build_rows_from_cache(&cached, &channel_names);

        // Assert: no TS file → source_video_file_id is None, file_size 0, file_exists false
        assert!(rows[0].source_video_file_id.is_none());
        assert_eq!(rows[0].file_size, 0);
        assert!(!rows[0].file_exists);
    }

    #[test]
    fn test_build_rows_from_cache_multiple_files_picks_ts() {
        // Arrange: TS and encoded files
        let ts = make_ts_video_file(10, 1);
        let encoded = dtvmgr_db::recorded::CachedVideoFile {
            id: 20,
            recorded_id: 1,
            name: String::from("encoded"),
            filename: Some(String::from("encoded.mp4")),
            file_type: String::from("encoded"),
            size: 500_000,
            file_exists: Some(true),
            file_checked_at: None,
        };
        let cached = vec![make_cached_pair(1, 100, vec![ts, encoded])];
        let mut channel_names = std::collections::HashMap::new();
        channel_names.insert(100_u64, String::from("NHK"));

        // Act
        let rows = build_rows_from_cache(&cached, &channel_names);

        // Assert: picks TS file
        assert_eq!(rows[0].source_video_file_id, Some(10));
        assert_eq!(rows[0].file_size, 2_000_000);
    }

    #[test]
    fn test_build_rows_from_cache_ts_file_exists_none() {
        // Arrange: TS file with file_exists = None → false
        let mut vf = make_ts_video_file(10, 1);
        vf.file_exists = None;
        let cached = vec![make_cached_pair(1, 100, vec![vf])];
        let channel_names = std::collections::HashMap::new();

        // Act
        let rows = build_rows_from_cache(&cached, &channel_names);

        // Assert
        assert!(!rows[0].file_exists);
    }

    #[test]
    fn test_build_rows_from_cache_ts_file_exists_false() {
        // Arrange: TS file with file_exists = Some(false)
        let mut vf = make_ts_video_file(10, 1);
        vf.file_exists = Some(false);
        let cached = vec![make_cached_pair(1, 100, vec![vf])];
        let channel_names = std::collections::HashMap::new();

        // Act
        let rows = build_rows_from_cache(&cached, &channel_names);

        // Assert
        assert!(!rows[0].file_exists);
    }

    #[test]
    fn test_build_rows_from_cache_optional_resolution_type_none() {
        // Arrange: video_resolution and video_type are None → default to ""
        let (mut item, files) = make_cached_pair(1, 100, vec![make_ts_video_file(10, 1)]);
        item.video_resolution = None;
        item.video_type = None;
        let cached = vec![(item, files)];
        let channel_names = std::collections::HashMap::new();

        // Act
        let rows = build_rows_from_cache(&cached, &channel_names);

        // Assert
        assert_eq!(rows[0].video_resolution, "");
        assert_eq!(rows[0].video_type, "");
    }

    #[test]
    fn test_build_rows_from_cache_recording_and_encoding_flags() {
        // Arrange
        let (mut item, files) = make_cached_pair(1, 100, vec![make_ts_video_file(10, 1)]);
        item.is_recording = true;
        item.is_encoding = true;
        let cached = vec![(item, files)];
        let channel_names = std::collections::HashMap::new();

        // Act
        let rows = build_rows_from_cache(&cached, &channel_names);

        // Assert
        assert!(rows[0].is_recording);
        assert!(rows[0].is_encoding);
    }

    // ── collect_files_to_check ───────────────────────────────────

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_collect_files_to_check_force() {
        // Arrange: file has recent check, but force=true
        let mut vf = make_ts_video_file(10, 1);
        vf.file_checked_at = Some(chrono::Utc::now().to_rfc3339());
        vf.file_exists = Some(true);
        let cached = vec![make_cached_pair(1, 100, vec![vf])];

        // Act
        let result = collect_files_to_check(&cached, true);

        // Assert: force always includes
        assert_eq!(result, vec![(10, 1)]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_collect_files_to_check_within_ttl() {
        // Arrange: file was checked 10 minutes ago (within 1h TTL)
        let mut vf = make_ts_video_file(10, 1);
        let recent = (chrono::Utc::now() - chrono::Duration::minutes(10)).to_rfc3339();
        vf.file_checked_at = Some(recent);
        vf.file_exists = Some(true);
        let cached = vec![make_cached_pair(1, 100, vec![vf])];

        // Act
        let result = collect_files_to_check(&cached, false);

        // Assert: within TTL → skip
        assert!(result.is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_collect_files_to_check_expired_ttl() {
        // Arrange: file was checked 2 hours ago (past 1h TTL)
        let mut vf = make_ts_video_file(10, 1);
        let old = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        vf.file_checked_at = Some(old);
        vf.file_exists = Some(true);
        let cached = vec![make_cached_pair(1, 100, vec![vf])];

        // Act
        let result = collect_files_to_check(&cached, false);

        // Assert: expired TTL → include
        assert_eq!(result, vec![(10, 1)]);
    }

    #[test]
    fn test_collect_files_to_check_never_checked() {
        // Arrange: file_exists and file_checked_at are None
        let mut vf = make_ts_video_file(10, 1);
        vf.file_exists = None;
        vf.file_checked_at = None;
        let cached = vec![make_cached_pair(1, 100, vec![vf])];

        // Act
        let result = collect_files_to_check(&cached, false);

        // Assert: never checked → include
        assert_eq!(result, vec![(10, 1)]);
    }

    #[test]
    fn test_collect_files_to_check_skips_non_ts() {
        // Arrange: only encoded file
        let encoded = dtvmgr_db::recorded::CachedVideoFile {
            id: 20,
            recorded_id: 1,
            name: String::from("encoded"),
            filename: Some(String::from("encoded.mp4")),
            file_type: String::from("encoded"),
            size: 500_000,
            file_exists: None,
            file_checked_at: None,
        };
        let cached = vec![make_cached_pair(1, 100, vec![encoded])];

        // Act
        let result = collect_files_to_check(&cached, false);

        // Assert: non-TS files are skipped
        assert!(result.is_empty());
    }

    #[test]
    fn test_collect_files_to_check_invalid_checked_at() {
        // Arrange: file_exists is Some but checked_at is unparseable
        let mut vf = make_ts_video_file(10, 1);
        vf.file_exists = Some(true);
        vf.file_checked_at = Some(String::from("not-a-date"));
        let cached = vec![make_cached_pair(1, 100, vec![vf])];

        // Act
        let result = collect_files_to_check(&cached, false);

        // Assert: invalid date → needs check
        assert_eq!(result, vec![(10, 1)]);
    }

    #[test]
    fn test_collect_files_to_check_checked_at_none_exists_some() {
        // Arrange: file_exists is Some but checked_at is None
        let mut vf = make_ts_video_file(10, 1);
        vf.file_exists = Some(true);
        vf.file_checked_at = None;
        let cached = vec![make_cached_pair(1, 100, vec![vf])];

        // Act
        let result = collect_files_to_check(&cached, false);

        // Assert: wildcard match → needs check
        assert_eq!(result, vec![(10, 1)]);
    }
}
