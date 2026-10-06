//! Starting forks from one long-lived thread, and following each until it ends.

use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::output::{Exit, ForkOutput};
use crate::stamp::ProcessStamp;
use crate::stop::{StopWaits, signal};
use crate::wrapper::Wrapper;

/// How much of a fork's standard error its [`Exit`] keeps.
const STDERR_TAIL: usize = 4096;
/// How long the end of a fork waits for its last output lines after the process ended: a
/// process the fork started may hold its output open longer.
const OUTPUT_WAIT: Duration = Duration::from_secs(1);

/// What a fork runs: its program, arguments, working directory and whole environment, of
/// which it inherits nothing else from the reviewer.
#[derive(Clone, Debug)]
pub struct ForkCommand {
    pub program: PathBuf,
    pub arguments: Vec<OsString>,
    pub directory: PathBuf,
    pub environment: Vec<(OsString, OsString)>,
}

/// The error of a spawn after the launcher's thread ended.
fn stopped() -> io::Error {
    io::Error::other("the fork launcher stopped")
}

/// Starts forks from a thread of its own, which lives until the launcher is dropped. Dropping
/// the launcher ends that thread, which sends every fork it started its parent-death signal.
pub struct Launcher {
    wrapper: Wrapper,
    /// How long a stop of each fork waits for it to end after each signal.
    waits: StopWaits,
    requests: Option<mpsc::Sender<Spawn>>,
    thread: Option<JoinHandle<()>>,
}

/// One fork to start, and where the launcher's thread answers.
struct Spawn {
    command: Command,
    reply: mpsc::Sender<io::Result<Child>>,
}

impl Launcher {
    /// A launcher whose forks start through `wrapper`.
    ///
    /// # Panics
    ///
    /// When the system cannot start the launcher's thread.
    pub fn start(wrapper: Wrapper) -> Self {
        Self::start_with(wrapper, StopWaits::default())
    }

    /// A launcher whose forks start through `wrapper`, and whose stops wait as `waits` says.
    pub(crate) fn start_with(wrapper: Wrapper, waits: StopWaits) -> Self {
        let (requests, received) = mpsc::channel::<Spawn>();
        let thread = thread::Builder::new()
            .name("fork launcher".into())
            .spawn(move || {
                for mut spawn in received {
                    let _ = spawn.reply.send(spawn.command.spawn());
                }
            })
            .expect("the fork launcher thread starts");
        Self {
            wrapper,
            waits,
            requests: Some(requests),
            thread: Some(thread),
        }
    }

    /// Starts `fork`, writes `input` to its standard input and closes it, and hands its
    /// standard output and its end to `output`. The fork stays in this process's group.
    pub fn spawn(
        &self,
        fork: ForkCommand,
        input: String,
        output: Box<dyn ForkOutput>,
    ) -> io::Result<RunningFork> {
        let mut command = Command::new(&self.wrapper.program);
        command
            .args(&self.wrapper.arguments)
            .arg(std::process::id().to_string())
            .arg(&fork.program)
            .args(&fork.arguments)
            .current_dir(&fork.directory)
            .env_clear()
            .envs(fork.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (reply, answer) = mpsc::channel();
        self.requests
            .as_ref()
            .ok_or_else(stopped)?
            .send(Spawn { command, reply })
            .map_err(|_| stopped())?;
        let child = answer.recv().map_err(|_| stopped())??;
        Ok(RunningFork::follow(child, input, output, self.waits))
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        drop(self.requests.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A started fork, followed by threads of its own until it ends. Dropping it leaves the fork
/// running: [`RunningFork::terminate`] stops it.
#[derive(Debug)]
pub struct RunningFork {
    stamp: ProcessStamp,
    exit: Arc<ExitSignal>,
    waits: StopWaits,
}

/// Set once the fork's process ended and was reaped.
#[derive(Debug, Default)]
struct ExitSignal {
    ended: Mutex<bool>,
    changed: Condvar,
}

impl ExitSignal {
    fn set(&self) {
        *self.ended.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.changed.notify_all();
    }

    fn is_set(&self) -> bool {
        *self.ended.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Waits at most `limit` for the end; whether it came.
    fn wait(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        let mut ended = self.ended.lock().unwrap_or_else(PoisonError::into_inner);
        while !*ended {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            ended = self
                .changed
                .wait_timeout(ended, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

impl RunningFork {
    fn follow(
        mut child: Child,
        input: String,
        output: Box<dyn ForkOutput>,
        waits: StopWaits,
    ) -> Self {
        let stamp = ProcessStamp::read(child.id());
        let exit = Arc::new(ExitSignal::default());
        if let Some(mut stdin) = child.stdin.take() {
            thread::spawn(move || {
                let _ = stdin.write_all(input.as_bytes());
            });
        }
        let output = Arc::new(Mutex::new(Some(output)));
        let lines = child.stdout.take().map(|stdout| {
            let output = Arc::clone(&output);
            let (done, finished) = mpsc::channel::<()>();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(output) = output
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .as_mut()
                    {
                        output.line(&line);
                    }
                }
                let _ = done.send(());
            });
            finished
        });
        let stderr = child.stderr.take().map(|stderr| {
            let output = Arc::clone(&output);
            let (tail, read) = mpsc::channel::<String>();
            thread::spawn(move || {
                let mut stderr = BufReader::new(stderr);
                let mut text = Vec::new();
                let mut line = Vec::new();
                while stderr
                    .read_until(b'\n', &mut line)
                    .is_ok_and(|read| read > 0)
                {
                    if let Some(output) = output
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .as_mut()
                    {
                        let read = String::from_utf8_lossy(&line);
                        output.error_line(read.trim_end_matches(['\n', '\r']));
                    }
                    text.append(&mut line);
                    if text.len() > 2 * STDERR_TAIL {
                        text.drain(..text.len() - STDERR_TAIL);
                    }
                }
                let start = text.len().saturating_sub(STDERR_TAIL);
                let _ = tail.send(String::from_utf8_lossy(&text[start..]).into_owned());
            });
            read
        });
        let ended = Arc::clone(&exit);
        thread::spawn(move || {
            let status = child
                .wait()
                .map_or_else(|error| error.to_string(), |status| status.to_string());
            ended.set();
            if let Some(finished) = lines {
                let _ = finished.recv_timeout(OUTPUT_WAIT);
            }
            let stderr = stderr
                .and_then(|read| read.recv_timeout(OUTPUT_WAIT).ok())
                .unwrap_or_default();
            let taken = output.lock().unwrap_or_else(PoisonError::into_inner).take();
            if let Some(output) = taken {
                output.ended(Exit { status, stderr });
            }
        });
        Self { stamp, exit, waits }
    }

    /// The fork's process, as a later reviewer can recognise it.
    pub fn stamp(&self) -> ProcessStamp {
        self.stamp
    }

    /// Whether the fork's process ended.
    pub(crate) fn has_exited(&self) -> bool {
        self.exit.is_set()
    }

    /// Stops the fork: SIGTERM, then SIGKILL when it still runs three seconds later. Returns
    /// once it ended, or two seconds after the SIGKILL.
    pub fn terminate(&self) {
        if self.has_exited() {
            return;
        }
        signal(self.stamp, nix::sys::signal::Signal::SIGTERM);
        if self.exit.wait(self.waits.term) {
            return;
        }
        signal(self.stamp, nix::sys::signal::Signal::SIGKILL);
        self.exit.wait(self.waits.kill);
    }
}

#[cfg(test)]
#[path = "launcher.tests.rs"]
mod tests;
