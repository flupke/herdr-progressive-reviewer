//! A run-ahead fork does not outlive the reviewer: started through `reviewer-control
//! fork-exec`, it gets SIGTERM when the reviewer dies, even of SIGKILL, outside any terminal.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use agent_fork::{Exit, ForkCommand, ForkOutput, Launcher, ProcessStamp, Wrapper};

/// Ignores a fork's output.
struct Ignored;

impl ForkOutput for Ignored {
    fn line(&mut self, _line: &str) {}

    fn ended(self: Box<Self>, _exit: Exit) {}
}

/// A stand-in reviewer: it starts one fork, prints the fork's process ID, then waits to be
/// killed.
#[test]
#[ignore = "runs in a child process of a_fork_ends_when_its_reviewer_is_killed"]
fn stand_in_reviewer() {
    if std::env::var_os("FORK_LIFETIME_REVIEWER").is_none() {
        return;
    }
    let launcher = Launcher::start(Wrapper {
        program: PathBuf::from(env!("CARGO_BIN_EXE_reviewer-control")),
        arguments: vec!["fork-exec".into()],
    });
    let fork = launcher
        .spawn(
            ForkCommand {
                program: "sleep".into(),
                arguments: vec!["60".into()],
                directory: std::env::temp_dir(),
                environment: std::env::vars_os().collect(),
            },
            String::new(),
            Box::new(Ignored),
        )
        .unwrap();
    println!("fork {}", fork.stamp().pid);
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[test]
fn a_fork_ends_when_its_reviewer_is_killed() {
    let mut reviewer = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "stand_in_reviewer", "--ignored", "--nocapture"])
        .env("FORK_LIFETIME_REVIEWER", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid: u32 = BufReader::new(reviewer.stdout.take().unwrap())
        .lines()
        .map_while(Result::ok)
        .find_map(|line| line.strip_prefix("fork ").map(|pid| pid.parse().unwrap()))
        .unwrap();
    let fork = ProcessStamp::of(pid).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fork.runs_with("60") && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(fork.runs_with("60"), "the fork runs its program");

    reviewer.kill().unwrap();
    reviewer.wait().unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while fork.is_running() {
        assert!(Instant::now() < deadline, "the fork outlived its reviewer");
        std::thread::sleep(Duration::from_millis(10));
    }
}
