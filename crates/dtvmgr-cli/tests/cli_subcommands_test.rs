#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

use assert_cmd::cargo_bin_cmd;
use predicates::prelude::predicate;

// ── db subcommands ─────────────────────────────────────────────

#[test]
fn test_db_sync_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["db", "sync", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--time-since"));
}

#[test]
fn test_db_list_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["db", "list", "--help"]).assert().success();
}

#[test]
fn test_db_normalize_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["db", "normalize", "--help"]).assert().success();
}

#[test]
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
fn test_tmdb_search_tv_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["tmdb", "search-tv", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--query"));
}

#[test]
fn test_tmdb_search_movie_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["tmdb", "search-movie", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--query"));
}

#[test]
fn test_tmdb_tv_details_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["tmdb", "tv-details", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--id"));
}

#[test]
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
fn test_jlse_channel_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["jlse", "channel", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--input"));
}

#[test]
fn test_jlse_param_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["jlse", "param", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--input"));
}

#[test]
fn test_jlse_run_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["jlse", "run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--input"));
}

#[test]
fn test_jlse_channel_missing_input() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["jlse", "channel"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--input"));
}

#[test]
fn test_jlse_run_missing_input() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["jlse", "run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--input"));
}
