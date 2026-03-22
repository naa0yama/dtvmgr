#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

use std::path::PathBuf;

use assert_cmd::cargo_bin_cmd;
use predicates::prelude::{PredicateBooleanExt, predicate};

// ── db subcommands ─────────────────────────────────────────────

#[test]
#[cfg_attr(miri, ignore)]
fn test_db_sync_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["db", "sync", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--time-since"));
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_db_list_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["db", "list", "--help"]).assert().success();
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_db_normalize_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["db", "normalize", "--help"]).assert().success();
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_db_tmdb_lookup_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["db", "tmdb-lookup", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--tids"));
}

// ── tmdb subcommands ───────────────────────────────────────────

#[test]
#[cfg_attr(miri, ignore)]
fn test_tmdb_search_tv_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["tmdb", "search-tv", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--query"));
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_tmdb_search_movie_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["tmdb", "search-movie", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--query"));
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_tmdb_tv_details_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["tmdb", "tv-details", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--id"));
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_tmdb_tv_season_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["tmdb", "tv-season", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--season"));
}

// ── jlse subcommands ───────────────────────────────────────────

#[test]
#[cfg_attr(miri, ignore)]
fn test_jlse_channel_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["jlse", "channel", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--input"));
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_jlse_param_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["jlse", "param", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--input"));
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_jlse_run_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["jlse", "run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--input"));
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_jlse_channel_missing_input() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["jlse", "channel"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--input"));
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_jlse_run_missing_input() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["jlse", "run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--input"));
}

// ── epgstation subcommands ────────────────────────────────────

#[test]
#[cfg_attr(miri, ignore)]
fn test_epgstation_encode_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["epgstation", "encode", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--keyword"));
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_epgstation_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["epgstation", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("encode"));
}

// ── completion subcommand ─────────────────────────────────────

#[test]
#[cfg_attr(miri, ignore)]
fn test_completion_bash() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_completion_zsh() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["completion", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_completion_fish() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["completion", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// ── init subcommand ───────────────────────────────────────────

#[test]
#[cfg_attr(miri, ignore)]
fn test_init_creates_new_config() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let config_path: PathBuf = dir.path().join("test_init_config.toml");
    assert!(!config_path.exists());

    // Act
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["--config", config_path.to_str().unwrap(), "init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Config created:"));

    // Assert
    assert!(config_path.exists());
    let contents = std::fs::read_to_string(&config_path).unwrap();
    assert!(!contents.is_empty());
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_init_already_up_to_date() {
    // Arrange: create config that matches the default template
    let dir = tempfile::tempdir().unwrap();
    let config_path: PathBuf = dir.path().join("test_init_uptodate.toml");

    // First, create the config via `init`
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["--config", config_path.to_str().unwrap(), "init"])
        .assert()
        .success();
    assert!(config_path.exists());

    // Act: run init again with the same config
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["--config", config_path.to_str().unwrap(), "init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Config already up to date"));
}
