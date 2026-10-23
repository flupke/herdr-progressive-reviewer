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

use herdr_client::protocol::{Agent, AgentPort, PaneId};

use crate::directory::{HookDirectory, HookedSession};
use crate::wire::{AgentEvent, Answer, Report, SessionSource};

/// How long a reviewer waits for the line of a hook that connected.
const READ_LIMIT: Duration = Duration::from_secs(1);

/// Counts the reviewers of this process, which each listen on a socket of their own.
static REVIEWERS: AtomicU64 = AtomicU64::new(0);

/// A reviewer's end of the hooks of the agents of its Herdr server: it listens on a socket of
/// its own in the server's [`HookDirectory`], one hook after another, hands each event of a pane
/// to what the reviewer expects there, and blocks the prompts submitted in a pane that the
/// reviewer does not expect there while it expects something. Dropping it stops listening and
/// removes its socket.
pub struct AgentHooks {
    directory: HookDirectory,
    socket: PathBuf,
    expected: Arc<Expectations>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// What the reviewer expects from the agent of each pane: several things at once, such as a
/// prompt it types and a session it has the agent resume.
#[derive(Default)]
struct Expectations {
    panes: Mutex<HashMap<PaneId, Vec<Awaited>>>,
    heard: Condvar,
    next: AtomicU64,
}

/// An expectation of the agent of a pane, and what its hooks said of it.
struct Awaited {
    id: u64,
    awaiting: Awaiting,
    /// Why a prompt submitted meanwhile is blocked, as the agent shows it.
    block: String,
    heard: Option<Heard>,
}

/// What an expectation waits for.
enum Awaiting {
    /// The agent resumes the session `session`, with a command of its own: every prompt
    /// submitted meanwhile is blocked.
    Resume { session: String },
    /// The prompt `text` the reviewer typed is submitted to the agent, on the session
    /// `session` when Herdr reports one: any other prompt submitted meanwhile is blocked.
    Prompt {
        session: Option<String>,
        text: String,
    },
}

/// What the agent's hooks said that an [`Expectation`] waited for, first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Heard {
    /// The agent resumed the session.
    Resumed,
    /// The text `prompt` was submitted to the agent while it resumed a session, and blocked. A
    /// command of the agent's own is no prompt: text submitted while the reviewer types one is
    /// text left in the agent's input box, which the command joined.
    Blocked { prompt: String },
    /// The reviewer's prompt was submitted to the agent as it was typed, alone.
    Submitted,
    /// The reviewer's prompt was submitted joined to the text `left` in the agent's input box,
    /// and blocked.
    Joined { left: String },
    /// The reviewer's prompt was submitted as it was typed, and blocked: the agent was resuming
    /// a session, or the prompt reached another session of the pane than Herdr reports.
    Stopped,
}

/// The reviewer expects something of the agent of a pane, and blocks the prompts it does not
/// expect there, until it drops this.
pub struct Expectation {
    expected: Arc<Expectations>,
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
        let expected = Arc::new(Expectations::default());
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
            directory: directory.clone(),
            socket,
            expected,
            stopped,
            thread: Some(thread),
        })
    }

    /// Whether `agent` runs the hooks: its process, still in its pane, started the session
    /// Herdr reports with them, as its hook recorded it. `port` reports the pane's processes.
    pub fn hooked(&self, port: &dyn AgentPort, agent: &Agent) -> bool {
        let Some(session) = &agent.agent_session else {
            return false;
        };
        let started: Vec<HookedSession> = self
            .directory
            .hooked_sessions(&agent.pane_id)
            .into_iter()
            .filter(|started| started.session == session.value && started.process.is_running())
            .collect();
        !started.is_empty()
            && port
                .pane_process_info(&agent.pane_id)
                .is_ok_and(|processes| {
                    processes.foreground_processes.iter().any(|process| {
                        started
                            .iter()
                            .any(|started| started.process.pid == process.pid)
                    })
                })
    }

    /// Expects the agent of `pane` to resume the session `session`, from now until the
    /// expectation is dropped, and meanwhile blocks each prompt submitted there, for `block`,
    /// which the agent shows: the reviewer arms it before it has the agent resume the session.
    pub fn expect_resume(&self, pane: &PaneId, session: &str, block: &str) -> Expectation {
        self.expect(
            pane,
            Awaiting::Resume {
                session: session.to_owned(),
            },
            block,
        )
    }

    /// Expects the prompt `text` to be submitted to the agent of `pane`, on the session
    /// `session` when Herdr reports one, from now until the expectation is dropped, and
    /// meanwhile blocks each other prompt submitted there, for `block`, which the agent shows:
    /// the reviewer arms it before it types the prompt. Text left in the agent's input box joins
    /// the typed prompt into one, which is blocked: the expectation hears it.
    pub fn expect_prompt(
        &self,
        pane: &PaneId,
        session: Option<&str>,
        text: &str,
        block: &str,
    ) -> Expectation {
        self.expect(
            pane,
            Awaiting::Prompt {
                session: session.map(str::to_owned),
                text: text.to_owned(),
            },
            block,
        )
    }

    fn expect(&self, pane: &PaneId, awaiting: Awaiting, block: &str) -> Expectation {
        let id = self.expected.next.fetch_add(1, Ordering::Relaxed);
        self.expected
            .lock()
            .entry(pane.clone())
            .or_default()
            .push(Awaited {
                id,
                awaiting,
                block: block.to_owned(),
                heard: None,
            });
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

impl Expectations {
    fn lock(&self) -> MutexGuard<'_, HashMap<PaneId, Vec<Awaited>>> {
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
            } => self.resumed(&report.pane, &session),
            AgentEvent::SessionStarted { .. } => {}
            AgentEvent::PromptSubmitted { prompt, session } => {
                let block = self.submitted(&report.pane, &prompt, session.as_deref());
                let Ok(mut answer) = serde_json::to_vec(&Answer { block }) else {
                    return;
                };
                answer.push(b'\n');
                let _ = (&mut &*stream).write_all(&answer);
            }
        }
    }

    /// The agent of `pane` resumed the session `session`.
    fn resumed(&self, pane: &PaneId, session: &str) {
        let mut panes = self.lock();
        let mut heard = false;
        for awaited in panes.get_mut(pane).into_iter().flatten() {
            if matches!(&awaited.awaiting, Awaiting::Resume { session: expected } if expected == session)
                && awaited.heard.is_none()
            {
                awaited.heard = Some(Heard::Resumed);
                heard = true;
            }
        }
        if heard {
            self.heard.notify_all();
        }
    }

    /// The text `prompt` was submitted to the agent of `pane`, on the session `session` if the
    /// hook says: why the reviewer blocks it, if it does.
    fn submitted(&self, pane: &PaneId, prompt: &str, session: Option<&str>) -> Option<String> {
        let mut panes = self.lock();
        let armed = panes.get_mut(pane)?;
        let block = armed
            .iter()
            .find(|awaited| awaited.blocks(prompt, session))
            .map(|awaited| awaited.block.clone());
        let mut heard = false;
        for awaited in armed.iter_mut().filter(|awaited| awaited.heard.is_none()) {
            awaited.heard = awaited.hears(prompt, session, block.is_some());
            heard |= awaited.heard.is_some();
        }
        if heard {
            self.heard.notify_all();
        }
        block
    }
}

impl Awaited {
    /// Whether this blocks the prompt `prompt`, submitted on the session `session`.
    fn blocks(&self, prompt: &str, session: Option<&str>) -> bool {
        match &self.awaiting {
            Awaiting::Resume { .. } => true,
            Awaiting::Prompt { text, .. } => {
                self.heard.is_none()
                    && (self.on_another_session(session) || prompt.trim() != text.trim())
            }
        }
    }

    /// Whether a prompt submitted on the session `session` reached another session than the
    /// one this expects, when both are known.
    fn on_another_session(&self, session: Option<&str>) -> bool {
        match &self.awaiting {
            Awaiting::Prompt {
                session: Some(expected),
                ..
            } => session.is_some_and(|session| session != expected),
            _ => false,
        }
    }

    /// What this hears of the prompt `prompt`, submitted on the session `session`, which the
    /// reviewer blocks if `blocked`. A prompt expectation hears every prompt that holds its
    /// text: the typed one alone, which [`Self::blocks`] lets go on, or joined to more, which it
    /// blocks.
    fn hears(&self, prompt: &str, session: Option<&str>, blocked: bool) -> Option<Heard> {
        match &self.awaiting {
            Awaiting::Resume { .. } => blocked.then(|| Heard::Blocked {
                prompt: prompt.to_owned(),
            }),
            // A prompt without the reviewer's is someone else's: the reviewer's may still come.
            Awaiting::Prompt { text, .. } if prompt.contains(text.trim()) => Some(if !blocked {
                Heard::Submitted
            } else if self.on_another_session(session) || prompt.trim() == text.trim() {
                Heard::Stopped
            } else {
                Heard::Joined {
                    left: prompt.replacen(text.trim(), "", 1).trim().to_owned(),
                }
            }),
            Awaiting::Prompt { .. } => None,
        }
    }
}

impl Expectation {
    /// What the agent's hooks said, once they said it, or `None` after `limit`.
    pub fn wait(&self, limit: Duration) -> Option<Heard> {
        let deadline = Instant::now() + limit;
        let mut panes = self.expected.lock();
        loop {
            let armed = panes
                .get(&self.pane)?
                .iter()
                .find(|awaited| awaited.id == self.id)?;
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
        if let Some(armed) = panes.get_mut(&self.pane) {
            armed.retain(|awaited| awaited.id != self.id);
            if armed.is_empty() {
                panes.remove(&self.pane);
            }
        }
    }
}

#[cfg(test)]
#[path = "listener.tests.rs"]
mod tests;
