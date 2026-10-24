//! The interface to the agent whose session run-ahead forks: Claude Code or Codex. What differs
//! between agents sits behind it, so the Explore session does not know which agent it drives.

use std::thread;
use std::time::Duration;

use herdr_client::client::{EventCanceller, HerdrClient};
use herdr_client::protocol::{Agent, AgentStatus, PaneId};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::record::TokenUsage;

/// Receives each status Herdr reports for a watched agent.
pub type StatusReport = Box<dyn Fn(AgentStatus) + Send + Sync>;

/// Watches an agent while it lives: dropping it stops the watch.
pub struct PaneWatch {
    _stop: Box<dyn Send>,
}

impl PaneWatch {
    /// A watch that `stop` ends when it is dropped.
    pub fn new(stop: impl Send + 'static) -> Self {
        Self {
            _stop: Box::new(stop),
        }
    }

    /// Reports to `report` each status `herdr` gives the agent of `pane`, on a thread of its
    /// own, until the watch is dropped. When Herdr drops the subscription, it reports the agent
    /// working, then subscribes again after `resubscribe`: the agent may have worked meanwhile,
    /// and the status that follows tells where it stands.
    pub fn herdr(
        herdr: &HerdrClient,
        pane: &PaneId,
        resubscribe: Duration,
        report: StatusReport,
    ) -> Self {
        let canceller = EventCanceller::default();
        let (herdr, pane, stop) = (herdr.clone(), pane.clone(), canceller.clone());
        thread::spawn(move || {
            let mut dropped = false;
            while !stop.is_cancelled() {
                if std::mem::take(&mut dropped) {
                    report(AgentStatus::Working);
                }
                let watched = herdr
                    .subscribe_agent_status(&pane, &stop)
                    .and_then(|statuses| {
                        statuses.forward(|status| {
                            report(status);
                            true
                        })
                    });
                match watched {
                    Ok(()) | Err(herdr_client::Error::Cancelled) => return,
                    Err(_) => {
                        dropped = true;
                        thread::sleep(resubscribe);
                    }
                }
            }
        });
        Self::new(CancelOnDrop(canceller))
    }
}

/// Ends a status watch when dropped.
struct CancelOnDrop(EventCanceller);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl std::fmt::Debug for PaneWatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PaneWatch")
    }
}

/// What one host keeps of a fork, or of where forks start, which only that host reads: Claude
/// Code's process, transcript directory and command; a Codex fork's daemon and turn. Run-ahead
/// holds it, and saves a fork's in the fork's record, at the record's top level, so that a
/// reopened reviewer's host finds it there.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct HostFields(Map<String, Value>);

impl HostFields {
    /// The field `name`, when it holds a `T`.
    pub fn get<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        T::deserialize(self.0.get(name)?).ok()
    }

    /// Whether the field `name` is set.
    pub fn has(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// Sets the field `name` to `value`.
    pub fn set(&mut self, name: &str, value: impl Serialize) {
        if let Ok(value) = serde_json::to_value(value) {
            self.0.insert(name.to_owned(), value);
        }
    }

    /// These fields, with those of `more` on top.
    #[must_use]
    pub fn with(mut self, more: Self) -> Self {
        self.0.extend(more.0);
        self
    }
}

/// Where forks of an agent's session start from: the session and its last conversation entry
/// when they were taken, and what the host needs to start forks that read the agent's prompt
/// cache.
#[derive(Clone, Debug)]
pub struct ForkPoint {
    /// The agent's session, which each fork copies.
    pub session: String,
    /// The session's last conversation entry, by ID: a later one means the session moved.
    pub entry: Option<String>,
    /// The model the session last answered with, which forks use, when the host knows it.
    pub model: Option<String>,
    /// What the host needs to start forks from here.
    pub host: HostFields,
}

/// A fork's session and the fields its host keeps of it, owned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedFork {
    /// The fork's session, which the host chose.
    pub session: String,
    /// What the host keeps of the fork.
    pub host: HostFields,
}

impl OwnedFork {
    /// The fork `trace` names.
    pub fn of(trace: ForkTrace<'_>) -> Self {
        Self {
            session: trace.session.to_owned(),
            host: trace.host.clone(),
        }
    }

    /// What stops the fork and removes what it left.
    pub fn trace(&self) -> ForkTrace<'_> {
        ForkTrace {
            session: &self.session,
            host: &self.host,
        }
    }
}

/// One fork to start.
#[derive(Clone, Copy, Debug)]
pub struct ForkStart<'a> {
    pub point: &'a ForkPoint,
    pub fork: ForkTrace<'a>,
}

/// How a fork ended.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ForkEnd {
    /// How it ended, in the host's words, with the end of its error output if it had one.
    pub exit: String,
    /// The tokens of its own requests, as far as it reported them.
    pub usage: TokenUsage,
    /// Whether its turn ended by itself.
    pub finished: bool,
}

/// What a host needs to stop a fork, switch to it and remove what it left: its session and the
/// fields the host keeps of it.
#[derive(Clone, Copy, Debug)]
pub struct ForkTrace<'a> {
    pub session: &'a str,
    pub host: &'a HostFields,
}

/// The switch of the pane's agent to a fork's session.
#[derive(Clone, Copy, Debug)]
pub struct SwitchTo<'a> {
    /// The pane whose agent switches.
    pub pane: &'a PaneId,
    /// The fork whose session the agent resumes.
    pub fork: ForkTrace<'a>,
}

/// Why the pane's agent was not switched to a fork's session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwitchFailure {
    pub error: String,
    /// Whether the agent was told to resume the fork's session: it may run it then.
    pub typed: bool,
}

impl SwitchFailure {
    /// A move that failed for `error` before the agent was told to resume the session.
    pub fn untyped(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            typed: false,
        }
    }

    /// The text left in the agent's input box joined the typed `/resume` into one prompt, which
    /// the reviewer's hook blocked: neither ran, and the agent did not move.
    fn joined() -> Self {
        Self::untyped(
            "text in the agent's input box joined the /resume, so the reviewer stopped both; \
             clear the agent's input box, then Retry",
        )
    }

    /// The reviewer's hook blocked `prompt`, submitted while the agent ran `command`, a
    /// `/resume`: text left in the input box joined the command, or another prompt came.
    pub fn blocked(prompt: &str, command: &str) -> Self {
        if prompt.contains(command) {
            Self::joined()
        } else {
            Self::prompt_meanwhile()
        }
    }

    /// Another prompt reached the agent while it resumed the session, which the reviewer's hook
    /// blocked: the resume may still run.
    fn prompt_meanwhile() -> Self {
        Self {
            error: "a prompt reached the agent while it resumed the session, and the reviewer \
                    stopped it"
                .into(),
            typed: true,
        }
    }
}

/// Why the reviewer's hook blocks a prompt submitted while the agent runs `command`, a
/// `/resume`: text left in the input box joined it.
pub fn resume_block(command: &str) -> String {
    format!(
        "The progressive reviewer stopped this prompt: it typed `{command}` while text waited \
         in the input box, and both would have run as one prompt. Clear the input box before \
         you Retry the answer in the reviewer."
    )
}

/// What run-ahead needs from the agent in the pane.
pub trait ForkHost: Send + Sync {
    /// Reports to `report` each status Herdr gives the agent of `pane`, until the returned watch
    /// is dropped.
    fn watch(&self, pane: &PaneId, report: StatusReport) -> PaneWatch;

    /// The session `agent` runs, as the host knows it; `None` when it knows none.
    fn session(&self, agent: &Agent) -> Option<String>;

    /// Why `agent` runs without the hooks that tell run-ahead what it does, in words the
    /// reviewer reads, such as an agent started before the reviewer's plugin was installed, or
    /// with the plugin disabled; `None` when it runs them. Run-ahead is off for it until it
    /// restarts. An agent run-ahead cannot fork anyway has none to miss.
    fn unhooked(&self, agent: &Agent) -> Option<String>;

    /// Whether the host can move `agent` to another session: it hears the agent's hooks.
    fn can_move(&self, agent: &Agent) -> bool;

    /// Why run-ahead never forks `agent`, whatever it tries: the agent is not one it forks, or
    /// the platform, the agent or the reviewer lacks what forks need. `None` when it may fork
    /// it.
    fn unforkable(&self, agent: &Agent) -> Option<String>;

    /// Where forks of `agent`'s session start from now, or why they could not be taken.
    fn point(&self, agent: &Agent) -> Result<ForkPoint, String>;

    /// The last conversation entry of the session `point` was taken from, as it is now.
    fn last_entry(&self, point: &ForkPoint) -> Option<String>;

    /// Makes the session of one fork of `point`, where nothing runs yet: run-ahead records it
    /// before the fork takes its turn.
    fn create(&self, point: &ForkPoint) -> Result<OwnedFork, String>;

    /// Has the fork `start.fork` take its turn on `prompt`; `ended` gets how it ended. Returns
    /// the fields the host keeps of it from now on, besides those it made it with.
    fn start(
        &self,
        start: ForkStart<'_>,
        prompt: String,
        ended: Box<dyn FnOnce(ForkEnd) + Send>,
    ) -> Result<HostFields, String>;

    /// Stops the fork `fork` if it still runs, then deletes its session, on a thread of its own;
    /// `done` runs once both are done.
    fn discard(&self, fork: ForkTrace<'_>, done: Box<dyn FnOnce() + Send>);

    /// Switches the agent of `switch.pane` to the session of the fork `switch.fork`, on a
    /// thread of its own. Once the fork's submit has its answer, or the fork ended, it stops
    /// the fork and keeps its session; then, with the agent idle, it has the agent resume the
    /// fork's session, and waits until it hears that the agent did and is ready for a prompt. A
    /// prompt submitted to the agent meanwhile, such as text left in its input box that the
    /// resume joined, is blocked and fails the switch. `done` gets the agent as Herdr then
    /// reports it, or why the switch failed.
    fn switch(
        &self,
        switch: SwitchTo<'_>,
        done: Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>,
    );

    /// Has the agent of `pane`, idle, which may run the session of the fork `from`, resume the
    /// session `session`, on a thread of its own, and waits as [`ForkHost::switch`] does once
    /// the fork stopped: it puts the agent back on the session its forks were taken from.
    /// `done` gets the agent as Herdr then reports it, or why it was not put back.
    fn resume(
        &self,
        pane: &PaneId,
        from: ForkTrace<'_>,
        session: &str,
        done: Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>,
    );

    /// Adds `line` to run-ahead's log, which says what the forks did.
    fn log(&self, line: &str);
}
