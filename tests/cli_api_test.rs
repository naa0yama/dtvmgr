#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

use assert_cmd::cargo_bin_cmd;
use predicates::prelude::predicate;

#[test]
fn test_api_prog_only_time_since() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["api", "prog", "--time-since", "2024-01-01"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "both --time-since and --time-until must be specified together",
        ));
}

#[test]
fn test_api_prog_only_time_until() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["api", "prog", "--time-until", "2024-01-31"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "both --time-since and --time-until must be specified together",
        ));
}

#[test]
fn test_api_prog_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["api", "prog", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--time-since"));
}

#[test]
fn test_api_titles_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["api", "titles", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--tids"));
}

#[test]
fn test_api_titles_missing_tids() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["api", "titles"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--tids"));
}
