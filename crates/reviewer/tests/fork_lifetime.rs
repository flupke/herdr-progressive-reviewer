//! A run-ahead fork does not outlive the reviewer: started through `reviewer-control
//! fork-exec`, it gets SIGTERM when the reviewer dies, even of SIGKILL, outside any terminal.

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use agent_fork::{Exit, ForkCommand, ForkOutput, Launcher, Wrapper};
use review_test_support::GUARD;

/// Ignores a fork's output.
struct Ignored;

impl ForkOutput for Ignored {
    fn line(&mut self, _line: &str) {}

    fn ended(self: Box<Self>, _exit: Exit) {}
}

/// A stand-in reviewer: it starts one fork, which holds the named pipe the test names open
/// for writing for as long as it runs, then waits to be killed.
#[test]
#[ignore = "runs in a child process of a_fork_ends_when_its_reviewer_is_killed"]
fn stand_in_reviewer() {
    let Some(pipe) = std::env::var_os("FORK_LIFETIME_REVIEWER") else {
        return;
    };
    let launcher = Launcher::start(Wrapper {
        program: PathBuf::from(env!("CARGO_BIN_EXE_reviewer-control")),
        arguments: vec!["fork-exec".into()],
    });
    let fork = launcher
        .spawn(
            ForkCommand {
                program: "sh".into(),
                arguments: vec!["-c".into(), r#"exec 3>"$0"; exec sleep 60"#.into(), pipe],
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
    let directory = tempfile::tempdir().unwrap();
    let pipe = directory.path().join("fork-alive");
    assert!(
        Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap()
            .success()
    );
    let mut reviewer = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "stand_in_reviewer", "--ignored", "--nocapture"])
        .env("FORK_LIFETIME_REVIEWER", &pipe)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let (opened, fork_started) = mpsc::channel();
    let (closed, fork_ended) = mpsc::channel();
    std::thread::spawn(move || {
        // Opening the pipe waits for the fork, and reading it ends once no process holds it.
        let mut fork = std::fs::File::open(pipe).unwrap();
        let _ = opened.send(());
        let _ = fork.read_to_end(&mut Vec::new());
        let _ = closed.send(());
    });
    fork_started
        .recv_timeout(GUARD)
        .expect("the fork runs its program");
    let started = BufReader::new(reviewer.stdout.take().unwrap())
        .lines()
        .map_while(Result::ok)
        .any(|line| line.starts_with("fork "));
    assert!(started, "the reviewer started the fork");

    reviewer.kill().unwrap();
    reviewer.wait().unwrap();

    fork_ended
        .recv_timeout(GUARD)
        .expect("the fork outlived its reviewer");
}
