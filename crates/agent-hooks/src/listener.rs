//! [`AgentHooks`]: a reviewer's end of the hooks of the agents of its Herdr server.

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use herdr_client::protocol::PaneId;

use crate::directory::HookDirectory;
use crate::wire::{AgentEvent, Answer, Report, SessionSource};

/// How long a reviewer waits for the line of a hook that connected.
const READ_LIMIT: Duration = Duration::from_secs(1);

/// Counts the reviewers of this process, which each listen on a socket of their own.
static REVIEWERS: AtomicU64 = AtomicU64::new(0);

/// A reviewer's end of the hooks of the agents of its Herdr server: it listens on a socket of
/// its own in the server's [`HookDirectory`], one hook after another, hands each event of a pane
/// to what the reviewer expects there, and blocks the prompts submitted in a pane while it
/// expects a session there. Dropping it stops listening and removes its socket.
pub struct AgentHooks {
    socket: PathBuf,
    expected: Arc<Expected>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// What the reviewer expects from the agent of each pane.
#[derive(Default)]
struct Expected {
    panes: Mutex<HashMap<PaneId, Armed>>,
    heard: Condvar,
    next: AtomicU64,
}

/// An expectation of the agent of a pane, and what its hooks said of it.
struct Armed {
    id: u64,
    /// The session the agent is expected to resume.
    session: String,
    /// Why a prompt submitted meanwhile is blocked, as the agent shows it.
    block: String,
    heard: Option<Heard>,
}

/// What the agent's hooks said that an [`Expectation`] waited for, first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Heard {
    /// The agent resumed the session.
    Resumed,
    /// The text `prompt` was submitted to the agent, and blocked. A command of the agent's own
    /// is no prompt: text submitted while the reviewer types one is a draft that met it.
    Blocked { prompt: String },
}

/// The reviewer expects the agent of a pane to resume a session, and blocks every prompt
/// submitted in that pane, until it drops this.
pub struct Expectation {
    expected: Arc<Expected>,
    pane: PaneId,
    id: u64,
}

impl AgentHooks {
    /// Listens in `directory`, once the sockets that stopped reviewers left there are removed.
    pub fn listen(directory: &HookDirectory) -> io::Result<Self> {
        directory.create()?;
        directory.remove_stale_sockets();
        let number = REVIEWERS.fetch_add(1, Ordering::Relaxed);
        let socket = directory.socket(std::process::id(), number);
        let listener = crate::address::bind(&socket)?;
        let expected = Arc::new(Expected::default());
        let stopped = Arc::new(AtomicBool::new(false));
        let thread = {
            let (expected, stopped) = (Arc::clone(&expected), Arc::clone(&stopped));
            thread::spawn(move || {
                for stream in listener.incoming() {
                    if stopped.load(Ordering::Acquire) {
                        return;
                    }
                    if let Ok(stream) = stream {
                        expected.take(&stream);
                    }
                }
            })
        };
        Ok(Self {
            socket,
            expected,
            stopped,
            thread: Some(thread),
        })
    }

    /// Expects the agent of `pane` to resume the session `session`, from now until the
    /// expectation is dropped, and meanwhile blocks each prompt submitted there, for `block`,
    /// which the agent shows: the reviewer arms it before it has the agent resume the session.
    pub fn expect_resume(&self, pane: &PaneId, session: &str, block: &str) -> Expectation {
        let id = self.expected.next.fetch_add(1, Ordering::Relaxed);
        self.expected.lock().insert(
            pane.clone(),
            Armed {
                id,
                session: session.to_owned(),
                block: block.to_owned(),
                heard: None,
            },
        );
        Expectation {
            expected: Arc::clone(&self.expected),
            pane: pane.clone(),
            id,
        }
    }
}

impl Drop for AgentHooks {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        // The thread waits for a connection: this one wakes it. Should the socket be gone, the
        // thread is left waiting, rather than this waiting for it forever.
        let woken = crate::address::connect(&self.socket).is_ok();
        let _ = fs::remove_file(&self.socket);
        if let Some(thread) = self.thread.take()
            && woken
        {
            let _ = thread.join();
        }
    }
}

impl Expected {
    fn lock(&self) -> MutexGuard<'_, HashMap<PaneId, Armed>> {
        self.panes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes the event that a hook sends on `stream`.
    fn take(&self, stream: &UnixStream) {
        let _ = stream.set_read_timeout(Some(READ_LIMIT));
        let mut line = String::new();
        if BufReader::new(stream).read_line(&mut line).is_err() {
            return;
        }
        // An event of a newer hook than this reviewer knows is not one it waits for.
        let Ok(report) = serde_json::from_str::<Report>(&line) else {
            return;
        };
        match report.event {
            AgentEvent::SessionStarted {
                session,
                source: SessionSource::Resume,
            } => {
                let mut panes = self.lock();
                if let Some(armed) = panes.get_mut(&report.pane)
                    && armed.session == session
                    && armed.heard.is_none()
                {
                    armed.heard = Some(Heard::Resumed);
                    self.heard.notify_all();
                }
            }
            AgentEvent::SessionStarted { .. } => {}
            AgentEvent::PromptSubmitted { prompt, .. } => {
                let mut panes = self.lock();
                let block = panes.get_mut(&report.pane).map(|armed| {
                    if armed.heard.is_none() {
                        armed.heard = Some(Heard::Blocked { prompt });
                        self.heard.notify_all();
                    }
                    armed.block.clone()
                });
                drop(panes);
                let Ok(mut answer) = serde_json::to_vec(&Answer { block }) else {
                    return;
                };
                answer.push(b'\n');
                let _ = (&mut &*stream).write_all(&answer);
            }
        }
    }
}

impl Expectation {
    /// What the agent's hooks said, once they said it, or `None` after `limit`.
    pub fn wait(&self, limit: Duration) -> Option<Heard> {
        let deadline = Instant::now() + limit;
        let mut panes = self.expected.lock();
        loop {
            let armed = panes.get(&self.pane).filter(|armed| armed.id == self.id)?;
            if let Some(heard) = &armed.heard {
                return Some(heard.clone());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            panes = self
                .expected
                .heard
                .wait_timeout(panes, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl Drop for Expectation {
    fn drop(&mut self) {
        let mut panes = self.expected.lock();
        if panes
            .get(&self.pane)
            .is_some_and(|armed| armed.id == self.id)
        {
            panes.remove(&self.pane);
        }
    }
}

#[cfg(test)]
#[path = "listener.tests.rs"]
mod tests;
