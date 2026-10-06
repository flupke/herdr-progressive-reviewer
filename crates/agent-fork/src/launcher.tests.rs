use std::ffi::OsString;
use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::stop::StopWaits;
use crate::stop_recorded;

/// A wrapper that runs the fork without arming a death signal: it drops the reviewer's
/// process ID the launcher passes and runs the rest.
fn plain_wrapper() -> Wrapper {
    Wrapper {
        program: "sh".into(),
        arguments: vec!["-c".into(), "shift; exec \"$@\"".into(), "sh".into()],
    }
}

fn shell(script: &str) -> ForkCommand {
    ForkCommand {
        program: "sh".into(),
        arguments: vec!["-c".into(), script.into()],
        directory: std::env::temp_dir(),
        environment: vec![(OsString::from("FORK_GREETING"), OsString::from("hello"))],
    }
}

/// Collects a fork's lines and sends them with its end.
struct Collected {
    lines: Vec<String>,
    sent: mpsc::Sender<(Vec<String>, Exit)>,
}

impl ForkOutput for Collected {
    fn line(&mut self, line: &str) {
        self.lines.push(line.to_owned());
    }

    fn ended(self: Box<Self>, exit: Exit) {
        let _ = self.sent.send((self.lines, exit));
    }
}

fn collected() -> (Box<Collected>, mpsc::Receiver<(Vec<String>, Exit)>) {
    let (sent, received) = mpsc::channel();
    (
        Box::new(Collected {
            lines: Vec::new(),
            sent,
        }),
        received,
    )
}

/// What a fork showed, as it came: a line of its standard output, then its end.
#[derive(Debug)]
enum Shown {
    Line(String),
    Ended(Exit),
}

/// Sends each line of a fork's standard output as it comes, then its end.
struct Streamed(mpsc::Sender<Shown>);

impl ForkOutput for Streamed {
    fn line(&mut self, line: &str) {
        let _ = self.0.send(Shown::Line(line.to_owned()));
    }

    fn ended(self: Box<Self>, exit: Exit) {
        let _ = self.0.send(Shown::Ended(exit));
    }
}

#[test]
fn a_fork_reads_its_input_sees_only_its_environment_and_reports_its_lines_and_end() {
    let launcher = Launcher::start(plain_wrapper());
    let (output, ended) = collected();

    let fork = launcher
        .spawn(
            shell("read line; echo \"$FORK_GREETING $line\"; echo \"home=${HOME:-none}\"; echo oops >&2; exit 3"),
            "world\n".into(),
            output,
        )
        .unwrap();

    let (lines, exit) = ended.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(lines, ["hello world", "home=none"]);
    assert_eq!(exit.status, "exit status: 3");
    assert_eq!(exit.stderr.trim(), "oops");
    assert!(fork.has_exited());
}

/// Sends each line of a fork's standard error as it comes.
struct Errors(mpsc::Sender<String>);

impl ForkOutput for Errors {
    fn line(&mut self, _line: &str) {}

    fn error_line(&mut self, line: &str) {
        let _ = self.0.send(line.to_owned());
    }

    fn ended(self: Box<Self>, _exit: Exit) {}
}

#[test]
fn a_fork_reports_each_line_of_its_standard_error_while_it_runs_and_keeps_its_end() {
    let launcher = Launcher::start(plain_wrapper());
    let (sent, received) = mpsc::channel();
    let fork = launcher
        .spawn(
            shell("echo first >&2; printf 'second\\r\\n' >&2; exec sleep 30"),
            String::new(),
            Box::new(Errors(sent)),
        )
        .unwrap();

    let lines: Vec<String> = (0..2)
        .map(|_| received.recv_timeout(Duration::from_secs(5)).unwrap())
        .collect();
    assert_eq!(lines, ["first", "second"]);
    assert!(!fork.has_exited(), "the lines came while the fork ran");
    fork.terminate();
}

#[test]
fn terminate_stops_a_fork_with_sigterm() {
    let launcher = Launcher::start(plain_wrapper());
    let (output, ended) = collected();
    let fork = launcher
        .spawn(shell("exec sleep 30"), String::new(), output)
        .unwrap();
    assert!(fork.stamp().is_running());

    fork.terminate();

    let (_, exit) = ended.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(exit.status, "signal: 15 (SIGTERM)");
    assert!(!fork.stamp().is_running());
}

#[test]
fn terminate_kills_a_fork_that_ignores_sigterm_once_its_wait_is_over() {
    let launcher = Launcher::start_with(
        plain_wrapper(),
        StopWaits {
            term: Duration::ZERO,
            ..StopWaits::default()
        },
    );
    let (sent, shown) = mpsc::channel();
    // The shell ignores SIGTERM, and so does the program it becomes.
    let fork = launcher
        .spawn(
            shell("trap '' TERM; echo ready; exec sleep 30"),
            String::new(),
            Box::new(Streamed(sent)),
        )
        .unwrap();
    // The trap is set once the shell prints.
    assert!(matches!(shown.recv().unwrap(), Shown::Line(line) if line == "ready"));

    fork.terminate();

    let Shown::Ended(exit) = shown.recv().unwrap() else {
        panic!("the end");
    };
    assert_eq!(exit.status, "signal: 9 (SIGKILL)");
}

#[test]
fn a_recorded_fork_is_stopped_only_when_it_still_runs_with_its_session() {
    // A process this test did not start, as one a killed reviewer left behind. It prints its
    // ID once its trap is set, then lets go of the output, which ends this command.
    let output = std::process::Command::new("sh")
        .args([
            "-c",
            "sh -c 'sleep 30 >/dev/null 2>&1 & trap \"kill $!; exit\" TERM; echo $$; exec >/dev/null 2>&1; wait' session-1 &",
        ])
        .output()
        .unwrap();
    let pid: u32 = String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let stamp = ProcessStamp::of(pid).unwrap();

    assert!(!stop_recorded(stamp, "session-2"));
    assert!(stamp.is_running());
    assert!(stop_recorded(stamp, "session-1"));
    assert!(!stamp.is_running());
}
