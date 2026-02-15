#![allow(clippy::unwrap_used)]
#![allow(missing_docs)]

use assert_cmd::cargo_bin_cmd;
use predicates::prelude::predicate;

#[test]
fn test_syoboi_prog_only_time_since() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["syoboi", "prog", "--time-since", "2024-01-01"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "both --time-since and --time-until must be specified together",
        ));
}

#[test]
fn test_syoboi_prog_only_time_until() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["syoboi", "prog", "--time-until", "2024-01-31"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "both --time-since and --time-until must be specified together",
        ));
}

#[test]
fn test_syoboi_prog_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["syoboi", "prog", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--time-since"));
}

#[test]
fn test_syoboi_titles_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["syoboi", "titles", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--tids"));
}

#[test]
fn test_syoboi_titles_missing_tids() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["syoboi", "titles"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--tids"));
}

#[test]
fn test_syoboi_channels_select_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["syoboi", "channels", "select", "--help"])
        .assert()
        .success();
}

#[test]
fn test_syoboi_channels_list_help() {
    // Arrange & Act & Assert
    let mut cmd = cargo_bin_cmd!("dtvmgr");
    cmd.args(["syoboi", "channels", "list", "--help"])
        .assert()
        .success();
}
