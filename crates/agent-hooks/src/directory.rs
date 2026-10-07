//! [`HookDirectory`]: where the reviewers of one Herdr server listen for the hooks of its
//! agents, and how a hook reaches them.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead as _, BufReader, Write as _};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agent_fork::ProcessStamp;
use herdr_client::protocol::PaneId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::wire::{Answer, Report};

/// The directory of one Herdr server under the user's runtime directory, readable by the user
/// alone: each open reviewer of that server listens there on a socket of its own. A hook reaches
/// the reviewers of its own server only, since the directory is named after Herdr's socket.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookDirectory(PathBuf);

/// The extension of a reviewer's socket.
const SOCKET: &str = "sock";

/// The extension of the record of a process's hooked session.
const RECORD: &str = "pane";

/// The session a process of a pane last started while it ran the reviewer's hooks, and the
/// process. A hook records it, for a reviewer opened later too: an agent whose process or
/// session no record names runs without the hooks.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct HookedSession {
    pub session: String,
    pub process: ProcessStamp,
}

/// How long a hook gives a reviewer to take an event.
const TELL_LIMIT: Duration = Duration::from_millis(200);

impl HookDirectory {
    /// The directory of the Herdr server whose socket is `herdr_socket`, under the runtime
    /// directory `runtime`, named after a digest of that socket's path.
    pub fn for_server(runtime: &Path, herdr_socket: &Path) -> Self {
        let digest = Sha256::digest(herdr_socket.as_os_str().as_encoded_bytes());
        Self(runtime.join("herdr-reviewer").join(hex(&digest[..6])))
    }

    /// The directory of the Herdr server this process runs under, from `XDG_RUNTIME_DIR` and
    /// `HERDR_SOCKET_PATH`; `None` without either.
    pub fn from_env() -> Option<Self> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|path| !path.is_empty())?;
        let herdr = std::env::var_os("HERDR_SOCKET_PATH").filter(|path| !path.is_empty())?;
        Some(Self::for_server(Path::new(&runtime), Path::new(&herdr)))
    }

    /// Hands `report` to every reviewer that listens in the directory. A reviewer that cannot
    /// be reached misses it.
    pub fn tell(&self, report: &Report) {
        let Some(line) = line(report) else {
            return;
        };
        for socket in self.sockets() {
            let _ = send(&socket, &line, TELL_LIMIT);
        }
    }

    /// Asks every reviewer that listens in the directory whether it blocks the prompt that
    /// `report` submits, within `limit` in all: why one blocks it, if one does. A reviewer that
    /// cannot be reached, or does not answer in time, does not block it.
    pub fn ask(&self, report: &Report, limit: Duration) -> Option<String> {
        let line = line(report)?;
        let deadline = Instant::now() + limit;
        self.sockets()
            .iter()
            .find_map(|socket| ask(socket, &line, deadline).ok().flatten())
    }

    /// Records that the agent of `pane` started `started` with the hooks, and forgets the
    /// processes of that pane that no longer run. Each process of a pane has a record of its
    /// own: a `claude` the agent runs from its shell inherits its pane, and runs the hooks too.
    pub fn record(&self, pane: &PaneId, started: &HookedSession) -> io::Result<()> {
        self.create()?;
        for (path, earlier) in self.records(pane) {
            if !earlier.process.is_running() {
                let _ = fs::remove_file(path);
            }
        }
        let path = self.0.join(format!(
            "{}{}.{RECORD}",
            record_prefix(pane),
            started.process.pid
        ));
        // Renamed into place, so that a reviewer never reads half a record.
        let written = path.with_extension(format!("{}.new", std::process::id()));
        fs::write(&written, serde_json::to_vec(started)?)?;
        fs::rename(&written, &path)
    }

    /// The session each process of `pane` last started with the hooks, as recorded, those of
    /// processes that ended since included.
    pub fn hooked_sessions(&self, pane: &PaneId) -> Vec<HookedSession> {
        self.records(pane)
            .into_iter()
            .map(|(_, started)| started)
            .collect()
    }

    /// The records of `pane`, by path.
    fn records(&self, pane: &PaneId) -> Vec<(PathBuf, HookedSession)> {
        let prefix = record_prefix(pane);
        let Ok(entries) = fs::read_dir(&self.0) else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == RECORD)
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with(&prefix))
            })
            .filter_map(|path| {
                let started = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
                Some((path, started))
            })
            .collect()
    }

    /// Creates the directory, readable by the user alone.
    pub(crate) fn create(&self) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.0)
    }

    /// The socket of the reviewer `number` of the process `process`.
    pub(crate) fn socket(&self, process: u32, number: u64) -> PathBuf {
        self.0.join(format!("{process}-{number}.{SOCKET}"))
    }

    /// The sockets of the reviewers that listen in the directory, and of those that stopped
    /// without removing theirs.
    pub(crate) fn sockets(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.0) else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == SOCKET)
            })
            .collect()
    }

    /// Removes the sockets of the processes that no longer run: their reviewers stopped without
    /// removing them.
    pub(crate) fn remove_stale_sockets(&self) {
        for socket in self.sockets() {
            let process = socket
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.split_once('-'))
                .and_then(|(process, _)| process.parse::<i32>().ok())
                .and_then(rustix::process::Pid::from_raw);
            if process.is_some_and(|process| {
                rustix::process::test_kill_process(process) == Err(rustix::io::Errno::SRCH)
            }) {
                let _ = fs::remove_file(socket);
            }
        }
    }
}

/// How the names of the records of `pane` start: its ID in hexadecimal, since a pane ID holds
/// any character, then a dash, before the process ID.
fn record_prefix(pane: &PaneId) -> String {
    format!("{}-", hex(pane.0.as_bytes()))
}

/// `bytes` in hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// `report` as the line a hook writes.
fn line(report: &Report) -> Option<Vec<u8>> {
    let mut line = serde_json::to_vec(report).ok()?;
    line.push(b'\n');
    Some(line)
}

/// Writes `line` to the reviewer that listens on `socket`, within `limit`; returns the
/// connection, on which the reviewer may answer.
fn send(socket: &Path, line: &[u8], limit: Duration) -> io::Result<std::os::unix::net::UnixStream> {
    let mut stream = crate::address::connect(socket)?;
    stream.set_write_timeout(Some(limit))?;
    stream.write_all(line)?;
    Ok(stream)
}

/// Writes `line` to the reviewer that listens on `socket`, and reads its answer, before
/// `deadline`: why it blocks the prompt, if it does.
fn ask(socket: &Path, line: &[u8], deadline: Instant) -> io::Result<Option<String>> {
    let left = || {
        Some(deadline.saturating_duration_since(Instant::now()))
            .filter(|left| !left.is_zero())
            .ok_or(io::ErrorKind::TimedOut)
    };
    let stream = send(socket, line, left()?)?;
    stream.set_read_timeout(Some(left()?))?;
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer)?;
    Ok(serde_json::from_str::<Answer>(&answer)?.block)
}
