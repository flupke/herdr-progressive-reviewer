//! `reviewer-control stats` prints the numbers of the saved Explore rounds of
//! the repository it runs in, and changes nothing.
use std::path::Path;
use std::process::{Command, Output};

use review_store::ReviewStore;
use review_test_support::GitFixture;

/// A synthetic answered round without a Challenger, started 2026-09-10.
const ANSWERED: &str = include_str!("../../review-explore-stats/testdata/answered.json");
/// A synthetic answered round with a Challenger, started 2026-09-20.
const CHALLENGER: &str = include_str!("../../review-explore-stats/testdata/challenger.json");

fn save(state: &Path, repository: &Path, json: &str) {
    let mut stored: serde_json::Value = serde_json::from_str(json).unwrap();
    let round: review_explore::ExploreRound =
        serde_json::from_value(stored["value"].take()).unwrap();
    let store = ReviewStore::open(state, repository).unwrap();
    let records = store
        .lock_explore(&round.exploration.comparison.checkpoint.review_unit)
        .unwrap();
    records.create_round(&round).unwrap();
}

fn stats(directory: &Path, environment: &[(&str, &Path)], arguments: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_reviewer-control"));
    command
        .arg("stats")
        .args(arguments)
        .current_dir(directory)
        .env_remove("HERDR_PLUGIN_STATE_DIR")
        .env_remove("XDG_STATE_HOME");
    for (key, value) in environment {
        command.env(key, value);
    }
    command.output().unwrap()
}

/// The values of every row with `label`, one row per table.
fn rows(output: &Output, label: &str) -> Vec<Vec<String>> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix(label))
        .filter(|rest| rest.starts_with("  "))
        .map(|rest| rest.split_whitespace().map(str::to_owned).collect())
        .collect()
}

#[test]
fn the_command_counts_the_rounds_of_the_repository_it_runs_in() {
    let repository = GitFixture::new();
    let state = tempfile::tempdir().unwrap();
    save(state.path(), repository.root(), ANSWERED);
    save(state.path(), repository.root(), CHALLENGER);
    let subdirectory = repository.root().join("src");
    std::fs::create_dir(&subdirectory).unwrap();

    let output = stats(
        &subdirectory,
        &[("HERDR_PLUGIN_STATE_DIR", state.path())],
        &[],
    );

    assert_eq!(rows(&output, "Rounds"), vec![vec!["2", "1", "1"]]);
}

#[test]
fn a_period_adds_the_numbers_of_the_rounds_started_in_it() {
    let repository = GitFixture::new();
    let state = tempfile::tempdir().unwrap();
    save(state.path(), repository.root(), ANSWERED);
    save(state.path(), repository.root(), CHALLENGER);

    let output = stats(
        repository.root(),
        &[("HERDR_PLUGIN_STATE_DIR", state.path())],
        &["--since", "2026-09-15", "--until", "2026-09-30"],
    );

    assert_eq!(
        rows(&output, "Rounds"),
        vec![vec!["2", "1", "1"], vec!["1", "1", "0"]]
    );
}

#[test]
fn rounds_with_a_challenger_count_its_proposals_by_result() {
    let repository = GitFixture::new();
    let state = tempfile::tempdir().unwrap();
    save(state.path(), repository.root(), ANSWERED);
    save(state.path(), repository.root(), CHALLENGER);

    let output = stats(
        repository.root(),
        &[("HERDR_PLUGIN_STATE_DIR", state.path())],
        &[],
    );

    for (label, count) in [
        ("Challenger's proposals", "5"),
        ("  asked", "1"),
        ("  merged with the implementer's", "1"),
        ("  retired by a fact", "2"),
        ("  kept for a later turn", "1"),
    ] {
        assert_eq!(
            rows(&output, label),
            vec![vec![count, count, "-"]],
            "{label}"
        );
    }
}

#[test]
fn without_a_plugin_state_directory_the_command_reads_the_user_state() {
    let repository = GitFixture::new();
    let state_home = tempfile::tempdir().unwrap();
    let state = state_home
        .path()
        .join("herdr/plugins/herdr.progressive-reviewer");
    save(&state, repository.root(), ANSWERED);

    let output = stats(
        repository.root(),
        &[("XDG_STATE_HOME", state_home.path())],
        &[],
    );

    assert_eq!(rows(&output, "Rounds"), vec![vec!["1", "0", "1"]]);
}

#[test]
fn a_repository_without_state_gets_zero_rounds_and_no_state() {
    let repository = GitFixture::new();
    let state = tempfile::tempdir().unwrap();
    let missing = state.path().join("plugin");

    let output = stats(
        repository.root(),
        &[("HERDR_PLUGIN_STATE_DIR", &missing)],
        &[],
    );

    assert_eq!(rows(&output, "Rounds"), vec![vec!["0", "0", "0"]]);
    assert!(!missing.exists(), "reading statistics must create nothing");
}

#[test]
fn an_unknown_time_fails_the_command() {
    let repository = GitFixture::new();
    let state = tempfile::tempdir().unwrap();

    let output = stats(
        repository.root(),
        &[("HERDR_PLUGIN_STATE_DIR", state.path())],
        &["--since", "last week"],
    );

    assert!(!output.status.success());
}
