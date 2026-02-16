//! dtvmgr - TV program data management CLI.

/// Application configuration (TOML).
mod config;
/// Terminal UI components.
mod tui;

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::instrument;
use tracing_subscriber::filter::EnvFilter;
#[cfg(not(feature = "otel"))]
use tracing_subscriber::fmt;
#[cfg(feature = "otel")]
use tracing_subscriber::layer::SubscriberExt;
#[cfg(feature = "otel")]
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::{AppConfig, resolve_config_path};
use crate::tui::run_channel_selector;
use crate::tui::state::{ChannelEntry, ChannelGroup};
use dtvmgr_api::syoboi::{
    LocalSyoboiApi, ProgLookupParams, SyoboiClient, SyoboiProgram, SyoboiTitle,
    lookup_all_programs, resolve_time_range,
};
use dtvmgr_api::tmdb::{LocalTmdbApi, SearchMovieParams, SearchTvParams, TmdbClient};
use dtvmgr_db::channels::{CachedChannel, CachedChannelGroup};
use dtvmgr_db::programs::CachedProgram;
use dtvmgr_db::titles::CachedTitle;
use dtvmgr_db::{
    load_channels, load_programs, load_titles, open_db, save_channel_groups, save_channels,
    upsert_programs, upsert_titles,
};

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
    /// Response language (default: "en-US").
    #[arg(long, default_value = "en-US")]
    language: String,
    /// Filter by year.
    #[arg(long)]
    year: Option<u32>,
}

/// Arguments for the `tmdb search-movie` subcommand.
#[derive(clap::Args)]
struct TmdbSearchMovieArgs {
    /// Search query (e.g. "すずめの戸締まり").
    #[arg(long, required = true)]
    query: String,
    /// Response language (default: "en-US").
    #[arg(long, default_value = "en-US")]
    language: String,
    /// Filter by year.
    #[arg(long)]
    year: Option<u32>,
}

/// Arguments for the `tmdb tv-details` subcommand.
#[derive(clap::Args)]
struct TmdbTvDetailsArgs {
    /// TMDB series ID.
    #[arg(long, required = true)]
    id: u64,
    /// Response language (default: "en-US").
    #[arg(long, default_value = "en-US")]
    language: String,
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
    /// Response language (default: "en-US").
    #[arg(long, default_value = "en-US")]
    language: String,
}

/// Runs the `syoboi prog` subcommand.
///
/// Falls back to `config.toml` selected channels when `--ch-ids` is not specified.
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
        ch_ids,
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
/// Returns `None` if no channels are specified.
fn resolve_ch_ids(ch_ids: Option<Vec<u32>>, dir: Option<&PathBuf>) -> Result<Option<Vec<u32>>> {
    if ch_ids.is_some() {
        return Ok(ch_ids);
    }

    let config_path = resolve_config_path(dir).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;
    if config.channels.selected.is_empty() {
        Ok(None)
    } else {
        tracing::info!(
            "Using {} channel(s) from config: {:?}",
            config.channels.selected.len(),
            config.channels.selected
        );
        Ok(Some(config.channels.selected))
    }
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
        title: t.title.clone(),
        short_title: t.short_title.clone(),
        title_yomi: t.title_yomi.clone(),
        title_en: t.title_en.clone(),
        cat: t.cat,
        title_flag: t.title_flag,
        first_year: t.first_year,
        first_month: t.first_month,
        keywords: t.keywords.clone(),
        sub_titles: t.sub_titles.clone(),
        last_update: t.last_update.clone(),
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

async fn run_db_sync(args: &DbSyncArgs, dir: Option<&PathBuf>) -> Result<()> {
    let client = build_syoboi_client()?;

    let range = resolve_time_range(args.time_since.as_deref(), args.time_until.as_deref())?;
    tracing::info!(
        "Time range: {} .. {}",
        range.start.format("%Y-%m-%d %H:%M:%S"),
        range.end.format("%Y-%m-%d %H:%M:%S"),
    );

    let ch_ids = resolve_ch_ids(args.ch_ids.clone(), dir)?;

    let params = ProgLookupParams {
        ch_ids,
        range: Some(range),
        ..ProgLookupParams::default()
    };

    tracing::info!("Fetching programs from Syoboi API...");
    let programs = lookup_all_programs(&client, &params)
        .await
        .context("failed to fetch programs")?;
    tracing::info!("Fetched {} programs", programs.len());

    // Extract unique TIDs and fetch titles in chunks
    let unique_tids: Vec<u32> = programs
        .iter()
        .map(|p| p.tid)
        .collect::<HashSet<u32>>()
        .into_iter()
        .collect();
    tracing::info!("Fetching titles for {} unique TIDs...", unique_tids.len());

    let all_titles = fetch_titles_chunked(&client, &unique_tids).await?;
    tracing::info!("Fetched {} titles total", all_titles.len());

    // Open DB and upsert
    let conn = open_db(dir).context("failed to open database")?;

    let cached_titles: Vec<CachedTitle> = all_titles.iter().map(to_cached_title).collect();
    let titles_changed = upsert_titles(&conn, &cached_titles).context("failed to upsert titles")?;
    tracing::info!(
        changed = titles_changed,
        unchanged = cached_titles.len().saturating_sub(titles_changed),
        "Titles upsert complete"
    );

    let valid_tids: HashSet<u32> = cached_titles.iter().map(|t| t.tid).collect();
    let cached_programs: Vec<CachedProgram> = programs
        .iter()
        .filter(|p| valid_tids.contains(&p.tid))
        .map(to_cached_program)
        .collect();
    let skipped = programs.len().saturating_sub(cached_programs.len());
    if skipped > 0 {
        tracing::warn!(skipped, "Skipped programs with missing title references");
    }
    let programs_changed =
        upsert_programs(&conn, &cached_programs).context("failed to upsert programs")?;
    tracing::info!(
        changed = programs_changed,
        unchanged = cached_programs.len().saturating_sub(programs_changed),
        "Programs upsert complete"
    );

    tracing::info!(
        "Sync complete: {} titles ({} changed), {} programs ({} changed)",
        cached_titles.len(),
        titles_changed,
        cached_programs.len(),
        programs_changed,
    );

    Ok(())
}

/// Builds a `TmdbClient` from the `TMDB_API_TOKEN` environment variable.
///
/// # Errors
///
/// Returns an error if `TMDB_API_TOKEN` is not set or the client fails to build.
#[instrument(skip_all)]
fn build_tmdb_client() -> Result<TmdbClient> {
    let api_token = std::env::var("TMDB_API_TOKEN")
        .context("TMDB_API_TOKEN environment variable is required")?;

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

/// Runs the `tmdb search-tv` subcommand.
///
/// # Errors
///
/// Returns an error if the TMDB client fails to build or the API request fails.
#[instrument(skip_all)]
async fn run_tmdb_search_tv(args: &TmdbSearchTvArgs) -> Result<()> {
    let client = build_tmdb_client()?;

    let mut params = SearchTvParams::new(&args.query).language(&args.language);
    if let Some(year) = args.year {
        params = params.year(year);
    }

    let response = client
        .search_tv(&params)
        .await
        .context("TMDB search/tv request failed")?;

    tracing::info!("Total results: {}", response.total_results);
    tracing::info!("ID\tName\t\t\tOrigLang\tCountry\t\tFirstAirDate");
    for result in &response.results {
        tracing::info!(
            "{}\t\t{}\t{}\t\t{}\t\t{}",
            result.id,
            result.name,
            result.original_language,
            result.origin_country.join(","),
            result.first_air_date.as_deref().unwrap_or("-"),
        );
    }

    Ok(())
}

/// Runs the `tmdb search-movie` subcommand.
///
/// # Errors
///
/// Returns an error if the TMDB client fails to build or the API request fails.
#[instrument(skip_all)]
async fn run_tmdb_search_movie(args: &TmdbSearchMovieArgs) -> Result<()> {
    let client = build_tmdb_client()?;

    let mut params = SearchMovieParams::new(&args.query).language(&args.language);
    if let Some(year) = args.year {
        params = params.year(year);
    }

    let response = client
        .search_movie(&params)
        .await
        .context("TMDB search/movie request failed")?;

    tracing::info!("Total results: {}", response.total_results);
    tracing::info!("ID\tTitle\t\t\tOrigLang\tReleaseDate");
    for result in &response.results {
        tracing::info!(
            "{}\t{}\t{}\t\t{}",
            result.id,
            result.title,
            result.original_language,
            result.release_date.as_deref().unwrap_or("-"),
        );
    }

    Ok(())
}

/// Runs the `tmdb tv-details` subcommand.
///
/// # Errors
///
/// Returns an error if the TMDB client fails to build or the API request fails.
#[instrument(skip_all)]
async fn run_tmdb_tv_details(args: &TmdbTvDetailsArgs) -> Result<()> {
    let client = build_tmdb_client()?;

    let details = client
        .tv_details(args.id, &args.language)
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
async fn run_tmdb_tv_season(args: &TmdbTvSeasonArgs) -> Result<()> {
    let client = build_tmdb_client()?;

    let season = client
        .tv_season(args.id, args.season, &args.language)
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
/// and saves selection to config.toml.
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
    let conn = open_db(dir).context("failed to open database")?;

    let cached_groups: Vec<CachedChannelGroup> = api_groups
        .iter()
        .map(|g| CachedChannelGroup {
            ch_gid: g.ch_gid,
            ch_group_name: g.ch_group_name.clone(),
            ch_group_order: g.ch_group_order,
        })
        .collect();
    save_channel_groups(&conn, &cached_groups).context("failed to cache channel groups")?;

    let cached_channels: Vec<CachedChannel> = api_channels
        .iter()
        .map(|ch| CachedChannel {
            ch_id: ch.ch_id,
            ch_gid: ch.ch_gid,
            ch_name: ch.ch_name.clone(),
        })
        .collect();
    save_channels(&conn, &cached_channels).context("failed to cache channels")?;

    // Load config
    let config_path = resolve_config_path(dir).context("failed to resolve config path")?;
    let config = AppConfig::load(&config_path).context("failed to load config")?;
    let initial_selected: BTreeSet<u32> = config.channels.selected.into_iter().collect();

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
        config.channels.selected = selected;
        config.save(&config_path).context("failed to save config")?;
        tracing::info!(
            "Saved {} selected channel(s) to {}",
            config.channels.selected.len(),
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

    if config.channels.selected.is_empty() {
        tracing::info!("No channels selected. Run `syoboi channels select` to choose channels.");
        return Ok(());
    }

    // Try to load names from DB cache
    let conn = open_db(dir);
    let cached_channels = conn
        .as_ref()
        .ok()
        .and_then(|c| load_channels(c).ok())
        .unwrap_or_default();

    tracing::info!("Selected channels ({}):", config.channels.selected.len());
    for ch_id in &config.channels.selected {
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
    let conn = open_db(dir).context("failed to open database")?;

    let titles = load_titles(&conn).context("failed to load titles")?;
    let programs = load_programs(&conn).context("failed to load programs")?;
    let channels = load_channels(&conn).context("failed to load channels")?;

    if titles.is_empty() {
        tracing::info!("No titles in database. Run `db sync` first.");
        return Ok(());
    }

    tracing::info!(
        "Loaded {} titles, {} programs, {} channels. Launching TUI...",
        titles.len(),
        programs.len(),
        channels.len()
    );

    crate::tui::title_viewer::run_title_viewer(&titles, &programs, channels)
        .context("title viewer TUI failed")?;

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
            TmdbSubcommands::SearchTv(args) => run_tmdb_search_tv(&args).await,
            TmdbSubcommands::SearchMovie(args) => run_tmdb_search_movie(&args).await,
            TmdbSubcommands::TvDetails(args) => run_tmdb_tv_details(&args).await,
            TmdbSubcommands::TvSeason(args) => run_tmdb_tv_season(&args).await,
        },
        Commands::Db(db) => match db.command {
            DbSubcommands::Sync(args) => run_db_sync(&args, cli.dir.as_ref()).await,
            DbSubcommands::List => run_db_list(cli.dir.as_ref()),
        },
    }
}
