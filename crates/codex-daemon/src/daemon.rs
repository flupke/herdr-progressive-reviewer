//! [`Daemon`]: where the app-server daemon of a pane's Codex agent listens.

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use herdr_client::protocol::PaneProcess;

use crate::follow::{Follow, SessionEvent};
use crate::rpc::{Incoming, Replier, Rpc};

/// The control socket of a Codex app-server daemon, which carries its JSON-RPC in WebSocket
/// text frames.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Daemon {
    socket: PathBuf,
}

/// Why the agent of a pane has no daemon the reviewer can join.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NoDaemon {
    #[error("no Codex process runs in the agent's pane")]
    NoCodex,
    #[error(
        "the Codex agent runs without the app-server daemon (--no-daemon), so the reviewer \
         cannot hear its turns: restart it without --no-daemon"
    )]
    WithoutDaemon,
    #[error("the Codex agent runs on the app server at {0}, which the reviewer does not join")]
    Remote(String),
    #[error("cannot read the Codex agent's {0}")]
    Unreadable(&'static str),
}

/// Where a Codex home keeps its daemon's control socket, a link to the socket itself.
const CONTROL_SOCKET: &str = "app-server-control/app-server-control.sock";

impl Daemon {
    /// The daemon of the Codex agent among `processes`, the foreground processes of its pane.
    pub fn of_pane(processes: &[PaneProcess]) -> Result<Self, NoDaemon> {
        // The npm package runs the native binary under a Node wrapper of the same name.
        let codex = processes
            .iter()
            .find(|process| process.name == "codex")
            .or_else(|| {
                processes.iter().find(|process| {
                    process
                        .argv
                        .iter()
                        .flatten()
                        .any(|argument| is_codex(argument))
                })
            })
            .ok_or(NoDaemon::NoCodex)?;
        let argv = codex
            .argv
            .as_deref()
            .ok_or(NoDaemon::Unreadable("command line"))?;
        let arguments = argv
            .iter()
            .position(|argument| is_codex(argument))
            .map_or(&[][..], |program| &argv[program + 1..]);
        let proc = PathBuf::from(format!("/proc/{}", codex.pid));
        let directory =
            fs::read_link(proc.join("cwd")).map_err(|_| NoDaemon::Unreadable("directory"))?;
        let environ =
            fs::read(proc.join("environ")).map_err(|_| NoDaemon::Unreadable("environment"))?;
        Self::of_command(arguments, &directory, |name| variable(&environ, name))
    }

    /// The daemon of a Codex TUI that runs with `arguments` in `directory`, with the
    /// environment variables `variable` gives.
    fn of_command(
        arguments: &[String],
        directory: &Path,
        variable: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Self, NoDaemon> {
        let mut remote = None;
        let mut arguments = arguments.iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--" => break,
                "--no-daemon" => return Err(NoDaemon::WithoutDaemon),
                "--remote" => remote = arguments.next().map(String::as_str),
                _ => {
                    if let Some(address) = argument.strip_prefix("--remote=") {
                        remote = Some(address);
                    }
                }
            }
        }
        let socket = match remote.map(|address| (address, address.strip_prefix("unix://"))) {
            None | Some((_, Some(""))) => {
                let home = variable("CODEX_HOME")
                    .map(PathBuf::from)
                    .or_else(|| variable("HOME").map(|home| PathBuf::from(home).join(".codex")))
                    .ok_or(NoDaemon::Unreadable("Codex home"))?;
                home.join(CONTROL_SOCKET)
            }
            Some((_, Some(path))) => PathBuf::from(path),
            Some((address, None)) => return Err(NoDaemon::Remote(address.to_owned())),
        };
        Ok(Self {
            socket: directory.join(socket),
        })
    }

    /// The daemon whose control socket is `socket`.
    pub fn at(socket: PathBuf) -> Self {
        Self { socket }
    }

    /// Where the daemon's control socket is.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Connects to the daemon, and hands `handler` what the daemon says from then on, on a
    /// thread of the connection's own, until the returned connection is dropped.
    pub fn connect(
        &self,
        handler: impl FnMut(Incoming, &Replier) + Send + 'static,
    ) -> Result<Rpc, String> {
        Rpc::connect(&self.socket, handler)
    }

    /// Joins the session `session` on a thread of its own, and hands `report` what it hears
    /// of it, in order, until the returned follow is dropped. When the daemon goes away, it
    /// tries to connect again every `retry`, and joins the session again once it can.
    pub fn follow(
        &self,
        session: &str,
        retry: Duration,
        report: impl FnMut(SessionEvent) + Send + 'static,
    ) -> Follow {
        Follow::start(self.socket.clone(), session.to_owned(), retry, report)
    }
}

/// Whether `argument` runs Codex: its program, or the script a wrapper runs.
fn is_codex(argument: &str) -> bool {
    Path::new(argument)
        .file_name()
        .is_some_and(|name| name == "codex")
}

/// The variable `name` of a process environment, as `/proc/<pid>/environ` holds it.
fn variable(environ: &[u8], name: &str) -> Option<OsString> {
    environ.split(|byte| *byte == 0).find_map(|entry| {
        entry
            .strip_prefix(name.as_bytes())?
            .strip_prefix(b"=")
            .map(|value| OsString::from_vec(value.to_vec()))
    })
}

#[cfg(test)]
#[path = "daemon.tests.rs"]
mod tests;
