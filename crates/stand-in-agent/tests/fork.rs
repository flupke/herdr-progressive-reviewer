//! The fork the stand-in plays when run-ahead starts it with Claude Code's arguments. Being an
//! integration test, it also has Cargo build the program for every test run of the workspace,
//! whose other tests run it too.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use review_test_support::stand_in::{
    EVENTS_VARIABLE, StandInCommand, StandInEvent, StandInEvents, StandInRole,
};
use stand_in_agent::{StandInDirectory, variables};

/// A fork run-ahead started as Claude Code's `arguments` say, on the prompt "Take the turn of
/// keep.", from the session `parent`, with its events, its directory and its Claude Code
/// configuration.
struct StartedFork {
    process: std::process::Child,
    events: StandInEvents,
    role: StandInRole,
    stand_in: StandInDirectory,
    configuration: std::path::PathBuf,
    _directory: tempfile::TempDir,
}

const ARGUMENTS: [&str; 6] = [
    "-p",
    "--resume",
    "parent",
    "--fork-session",
    "--session-id",
    "fork",
];

fn start_fork() -> StartedFork {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("events.sock");
    let events = StandInEvents::listen(&socket);
    let configuration = directory.path().join("claude-config");
    let parent = stand_in_agent::transcript(&configuration, "parent");
    fs::create_dir_all(parent.parent().unwrap()).unwrap();
    fs::write(&parent, "{\"type\":\"user\",\"uuid\":\"start\"}\n").unwrap();
    let stand_in = StandInDirectory::new(directory.path().join("agent"));
    let mut process = Command::new(env!("CARGO_BIN_EXE_stand-in-agent"))
        .args(ARGUMENTS)
        .env(EVENTS_VARIABLE, &socket)
        .env(variables::DIRECTORY, stand_in.path())
        .env("CLAUDE_CONFIG_DIR", &configuration)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(b"Take the turn of keep.")
        .unwrap();
    StartedFork {
        process,
        events,
        role: StandInRole::Fork {
            session: "fork".into(),
        },
        stand_in,
        configuration,
        _directory: directory,
    }
}

/// The lines of Claude Code's stream that a fork printed.
fn stream(output: &std::process::Output) -> Vec<serde_json::Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_fork_copies_its_parent_s_session_and_its_output_answers_its_submit() {
    let StartedFork {
        process: fork,
        events,
        role,
        stand_in,
        configuration,
        _directory,
    } = start_fork();

    let started = events.wait_for("the fork's start", |reported| {
        matches!(reported.event, StandInEvent::ForkStarted { .. })
    });
    events.send(&role, &StandInCommand::Submit);
    events.wait_for("the fork's submit", |reported| {
        reported.event == StandInEvent::ForkSubmitted
    });
    events.send(&role, &StandInCommand::End);
    let output = fork.wait_with_output().unwrap();

    assert_eq!(started.from, role);
    assert_eq!(
        started.event,
        StandInEvent::ForkStarted {
            parent: "parent".into(),
            prompt: "Take the turn of keep.".into(),
        }
    );
    let session = fs::read_to_string(stand_in_agent::transcript(&configuration, "fork")).unwrap();
    assert!(
        session.starts_with("{\"type\":\"user\",\"uuid\":\"start\"}\n"),
        "{session}"
    );
    assert_eq!(
        fs::read_to_string(stand_in.fork_record("fork", "args")).unwrap(),
        ARGUMENTS.join("\n")
    );
    // Claude Code's stream: the submit, its result, then the end of the turn.
    let stream = stream(&output);
    let submit = stream
        .iter()
        .position(|line| {
            line["message"]["content"][0]["name"] == "mcp__herdr_reviewer__submit_question"
        })
        .expect("the fork's submit");
    assert_eq!(
        stream[submit + 1]["message"]["content"][0]["tool_use_id"],
        stream[submit]["message"]["content"][0]["id"]
    );
    assert_eq!(stream.last().unwrap()["type"], "result");
    events.wait_for("the fork's end", |reported| {
        reported.event == StandInEvent::ForkFinished
    });
}

#[test]
fn a_fork_told_to_crash_exits_with_a_failure_and_no_result() {
    let StartedFork {
        process: fork,
        events,
        role,
        // Kept to the end: the fork's files are under it.
        _directory,
        ..
    } = start_fork();
    events.wait_for("the fork's start", |reported| {
        matches!(reported.event, StandInEvent::ForkStarted { .. })
    });

    events.send(&role, &StandInCommand::Crash);

    let output = fork.wait_with_output().unwrap();
    assert!(!output.status.success(), "{:?}", output.status);
    assert!(
        stream(&output).iter().all(|line| line["type"] != "result"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    events.wait_for("the fork's exit", |reported| {
        reported.from == role && reported.event == StandInEvent::Disconnected
    });
    assert!(
        !events
            .received()
            .iter()
            .any(|reported| reported.event == StandInEvent::ForkFinished)
    );
}
