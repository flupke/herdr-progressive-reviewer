//! [`HookDirectory`]: where the reviewers of one Herdr server listen for the hooks of its
//! agents, and how a hook reaches them.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead as _, BufReader, Write as _};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::wire::{Answer, Report};

/// The directory of one Herdr server under the user's runtime directory, readable by the user
/// alone: each open reviewer of that server listens there on a socket of its own. A hook reaches
/// the reviewers of its own server only, since the directory is named after Herdr's socket.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookDirectory(PathBuf);

/// The extension of a reviewer's socket.
const SOCKET: &str = "sock";

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
        self.sockets().iter().find_map(|socket| {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            ask(socket, &line, left).ok().flatten()
        })
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

/// Writes `line` to the reviewer that listens on `socket`, and reads its answer, within
/// `limit`: why it blocks the prompt, if it does.
fn ask(socket: &Path, line: &[u8], limit: Duration) -> io::Result<Option<String>> {
    let stream = send(socket, line, limit)?;
    stream.set_read_timeout(Some(limit))?;
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer)?;
    Ok(serde_json::from_str::<Answer>(&answer)?.block)
}
