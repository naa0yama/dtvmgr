//! dtvmgr - TV program data management CLI.

/// Application configuration (TOML).
mod config;
/// Terminal UI components.
mod tui;

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use chrono::{Duration, Local, NaiveDateTime};
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
    LocalSyoboiApi, ProgLookupParams, SyoboiClient, TimeRange, lookup_all_programs,
};
use dtvmgr_api::tmdb::{LocalTmdbApi, SearchMovieParams, SearchTvParams, TmdbClient};
use dtvmgr_db::channels::{CachedChannel, CachedChannelGroup};
use dtvmgr_db::{load_channels, open_db, save_channel_groups, save_channels};

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

/// Tries full datetime formats first, returns `None` if both fail.
fn try_full_datetime(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .ok()
}

/// Converts a datetime string for `--time-since` (date-only defaults to `00:00:00`).
///
/// Accepts: `%Y-%m-%dT%H:%M:%S`, `%Y-%m-%d %H:%M:%S`, `%Y-%m-%d`.
///
/// # Errors
///
/// Returns an error if the string does not match any known format.
fn to_naive_datetime_since(s: &str) -> Result<NaiveDateTime> {
    if let Some(dt) = try_full_datetime(s) {
        return Ok(dt);
    }
    NaiveDateTime::parse_from_str(&format!("{s}T00:00:00"), "%Y-%m-%dT%H:%M:%S")
        .with_context(|| format!("invalid datetime format: {s}"))
}

/// Converts a datetime string for `--time-until` (date-only defaults to `23:59:59`).
///
/// Accepts: `%Y-%m-%dT%H:%M:%S`, `%Y-%m-%d %H:%M:%S`, `%Y-%m-%d`.
///
/// # Errors
///
/// Returns an error if the string does not match any known format.
fn to_naive_datetime_until(s: &str) -> Result<NaiveDateTime> {
    if let Some(dt) = try_full_datetime(s) {
        return Ok(dt);
    }
    NaiveDateTime::parse_from_str(&format!("{s}T23:59:59"), "%Y-%m-%dT%H:%M:%S")
        .with_context(|| format!("invalid datetime format: {s}"))
}

/// Resolves time range from CLI arguments using local timezone.
///
/// # Errors
///
/// Returns an error if only one of `--time-since` / `--time-until` is specified.
fn resolve_time_range(args: &ProgArgs) -> Result<TimeRange> {
    match (&args.time_since, &args.time_until) {
        (None, None) => {
            let now = Local::now().naive_local();
            let start = now
                .checked_sub_signed(Duration::days(1))
                .context("failed to compute start time")?;
            let end = now
                .checked_add_signed(Duration::days(1))
                .context("failed to compute end time")?;
            Ok(TimeRange::new(start, end))
        }
        (Some(since), Some(until)) => {
            let start = to_naive_datetime_since(since)?;
            let end = to_naive_datetime_until(until)?;
            Ok(TimeRange::new(start, end))
        }
        _ => {
            bail!("both --time-since and --time-until must be specified together");
        }
    }
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

    let range = resolve_time_range(args)?;
    tracing::info!(
        "Time range: {} .. {}",
        range.start.format("%Y-%m-%d %H:%M:%S"),
        range.end.format("%Y-%m-%d %H:%M:%S"),
    );

    let ch_ids = if args.ch_ids.is_some() {
        args.ch_ids.clone()
    } else {
        let config_path = resolve_config_path(dir).context("failed to resolve config path")?;
        let config = AppConfig::load(&config_path).context("failed to load config")?;
        if config.channels.selected.is_empty() {
            None
        } else {
            tracing::info!(
                "Using {} channel(s) from config: {:?}",
                config.channels.selected.len(),
                config.channels.selected
            );
            Some(config.channels.selected)
        }
    };

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
        .lookup_titles(&args.tids)
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
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn test_to_naive_datetime_since_iso_format() {
        // Arrange & Act
        let dt = to_naive_datetime_since("2024-01-15T09:30:00").unwrap();

        // Assert
        assert_eq!(dt.to_string(), "2024-01-15 09:30:00");
    }

    #[test]
    fn test_to_naive_datetime_until_iso_format() {
        // Arrange & Act
        let dt = to_naive_datetime_until("2024-01-15T09:30:00").unwrap();

        // Assert
        assert_eq!(dt.to_string(), "2024-01-15 09:30:00");
    }

    #[test]
    fn test_to_naive_datetime_since_space_format() {
        // Arrange & Act
        let dt = to_naive_datetime_since("2024-01-15 09:30:00").unwrap();

        // Assert
        assert_eq!(dt.to_string(), "2024-01-15 09:30:00");
    }

    #[test]
    fn test_to_naive_datetime_until_space_format() {
        // Arrange & Act
        let dt = to_naive_datetime_until("2024-01-15 09:30:00").unwrap();

        // Assert
        assert_eq!(dt.to_string(), "2024-01-15 09:30:00");
    }

    #[test]
    fn test_to_naive_datetime_since_date_only() {
        // Arrange & Act
        let dt = to_naive_datetime_since("2024-01-15").unwrap();

        // Assert
        assert_eq!(dt.to_string(), "2024-01-15 00:00:00");
    }

    #[test]
    fn test_to_naive_datetime_until_date_only() {
        // Arrange & Act
        let dt = to_naive_datetime_until("2024-01-15").unwrap();

        // Assert
        assert_eq!(dt.to_string(), "2024-01-15 23:59:59");
    }

    #[test]
    fn test_to_naive_datetime_since_invalid() {
        // Arrange & Act
        let result = to_naive_datetime_since("not-a-date");

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn test_to_naive_datetime_until_invalid() {
        // Arrange & Act
        let result = to_naive_datetime_until("not-a-date");

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_time_range_both_none() {
        // Arrange
        let args = ProgArgs {
            time_since: None,
            time_until: None,
            ch_ids: None,
        };

        // Act
        let range = resolve_time_range(&args).unwrap();

        // Assert: range should span roughly 2 days
        let diff = range.end - range.start;
        assert_eq!(diff.num_days(), 2);
    }

    #[test]
    fn test_resolve_time_range_both_some() {
        // Arrange
        let args = ProgArgs {
            time_since: Some(String::from("2024-01-01")),
            time_until: Some(String::from("2024-01-31")),
            ch_ids: None,
        };

        // Act
        let range = resolve_time_range(&args).unwrap();

        // Assert
        assert_eq!(range.start.to_string(), "2024-01-01 00:00:00");
        assert_eq!(range.end.to_string(), "2024-01-31 23:59:59");
    }

    #[test]
    fn test_resolve_time_range_only_since() {
        // Arrange
        let args = ProgArgs {
            time_since: Some(String::from("2024-01-01")),
            time_until: None,
            ch_ids: None,
        };

        // Act
        let result = resolve_time_range(&args);

        // Assert
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("both --time-since and --time-until must be specified together")
        );
    }

    #[test]
    fn test_resolve_time_range_only_until() {
        // Arrange
        let args = ProgArgs {
            time_since: None,
            time_until: Some(String::from("2024-01-31")),
            ch_ids: None,
        };

        // Act
        let result = resolve_time_range(&args);

        // Assert
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("both --time-since and --time-until must be specified together")
        );
    }
}
