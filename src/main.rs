//! dtvmgr - TV program data management CLI.

/// Library modules.
pub mod libs;

use anyhow::{Context, Result, bail};
use chrono::{Duration, Local, NaiveDateTime};
use clap::{Parser, Subcommand};
use tracing_subscriber::filter::EnvFilter;
#[cfg(not(feature = "otel"))]
use tracing_subscriber::fmt;
#[cfg(feature = "otel")]
use tracing_subscriber::layer::SubscriberExt;
#[cfg(feature = "otel")]
use tracing_subscriber::util::SubscriberInitExt;

use crate::libs::syoboi::{
    LocalSyoboiApi, ProgLookupParams, SyoboiClient, TimeRange, lookup_all_programs,
};
use crate::libs::tmdb::{LocalTmdbApi, SearchMovieParams, SearchTvParams, TmdbClient};

/// CLI argument parser.
#[derive(Parser)]
#[command(about, version)]
struct Cli {
    /// Subcommand to run.
    #[command(subcommand)]
    command: Commands,
}

/// Available subcommands.
#[derive(Subcommand)]
enum Commands {
    /// Query Syoboi Calendar API.
    Api(ApiCommand),
    /// Query TMDB API.
    Tmdb(TmdbCommand),
}

/// Arguments for the `api` subcommand.
#[derive(clap::Args)]
struct ApiCommand {
    /// API subcommand to run.
    #[command(subcommand)]
    command: ApiSubcommands,
}

/// Available API subcommands.
#[derive(Subcommand)]
enum ApiSubcommands {
    /// Query program schedule data (`ProgLookup`).
    Prog(ProgArgs),
    /// Query title data (`TitleLookup`).
    Titles(TitlesArgs),
}

/// Arguments for the `api prog` subcommand.
#[derive(clap::Args)]
struct ProgArgs {
    /// Start datetime (default: now - 1 day).
    /// Formats: "2024-01-01T00:00:00", "2024-01-01 00:00:00", "2024-01-01".
    #[arg(long)]
    time_since: Option<String>,

    /// End datetime (default: now + 1 day). Same formats as --time-since.
    #[arg(long)]
    time_until: Option<String>,

    /// Comma-separated channel IDs (e.g. "1,7,19").
    #[arg(long, value_delimiter = ',')]
    ch_ids: Option<Vec<u32>>,
}

/// Arguments for the `api titles` subcommand.
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

/// Runs the `api prog` subcommand.
///
/// # Errors
///
/// Returns an error if the API client fails to build, time range is invalid,
/// or the API request fails.
async fn run_api_prog(args: &ProgArgs) -> Result<()> {
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

    let params = ProgLookupParams {
        ch_ids: args.ch_ids.clone(),
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

/// Runs the `api titles` subcommand.
///
/// # Errors
///
/// Returns an error if the API client fails to build or the API request fails.
async fn run_api_titles(args: &TitlesArgs) -> Result<()> {
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
            .init();
    }

    #[cfg(feature = "otel")]
    {
        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        let fmt_layer = tracing_subscriber::fmt::layer();

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
        Commands::Api(api) => match api.command {
            ApiSubcommands::Prog(args) => run_api_prog(&args).await,
            ApiSubcommands::Titles(args) => run_api_titles(&args).await,
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
