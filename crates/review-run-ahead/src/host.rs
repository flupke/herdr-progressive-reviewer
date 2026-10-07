//! The interface to the agent whose session run-ahead forks: Claude Code for now. What differs
//! between agents sits behind it, so the Explore session does not know which agent it drives.

use std::path::{Path, PathBuf};

use agent_fork::{ForkCommand, ProcessStamp};
use herdr_client::protocol::{Agent, AgentStatus, PaneId};

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
}

impl std::fmt::Debug for PaneWatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PaneWatch")
    }
}

/// Where forks of an agent's session start from: the session and its last conversation entry
/// when they were taken, and how the agent runs, so that forks start with its exact flags and
/// read its prompt cache.
#[derive(Clone, Debug)]
pub struct ForkPoint {
    /// The agent's session, which each fork copies.
    pub session: String,
    /// The session's last conversation entry, by ID: a later one means the session moved.
    pub entry: Option<String>,
    /// Where the agent keeps its transcripts, the forks' included.
    pub transcripts: PathBuf,
    /// The model the session last answered with, which forks use.
    pub model: Option<String>,
    /// The agent's own command, without what picks its session or its interactive mode.
    pub command: ForkCommand,
}

/// One fork to start.
#[derive(Debug)]
pub struct ForkStart<'a> {
    pub point: &'a ForkPoint,
    /// The session ID the fork takes, chosen by the caller.
    pub session: &'a str,
    /// The prompt the fork takes its turn on.
    pub prompt: String,
}

/// How a fork ended.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ForkEnd {
    /// The process's end, as the system words it, with the end of its error output.
    pub exit: String,
    /// The tokens of its own requests, as far as it reported them.
    pub usage: TokenUsage,
    /// Whether its turn ended by itself, rather than with the process.
    pub finished: bool,
}

/// What stops a fork and removes what it left: its session, its process, and where its
/// transcript is.
#[derive(Clone, Copy, Debug)]
pub struct ForkTrace<'a> {
    pub session: &'a str,
    pub process: Option<ProcessStamp>,
    pub transcripts: &'a Path,
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

/// What run-ahead needs from the agent in the pane.
pub trait ForkHost: Send + Sync {
    /// Reports to `report` each status Herdr gives the agent of `pane`, until the returned watch
    /// is dropped.
    fn watch(&self, pane: &PaneId, report: StatusReport) -> PaneWatch;

    /// Where forks of `agent`'s session start from now, or why it cannot be forked.
    fn point(&self, agent: &Agent) -> Result<ForkPoint, String>;

    /// The last conversation entry of the session `point` was taken from, as it is now.
    fn last_entry(&self, point: &ForkPoint) -> Option<String>;

    /// Starts the fork `start`; `ended` gets how it ended. Returns its process.
    fn start(
        &self,
        start: ForkStart<'_>,
        ended: Box<dyn FnOnce(ForkEnd) + Send>,
    ) -> Result<ProcessStamp, String>;

    /// Stops the fork `fork` if its process still runs, then deletes its transcript, on a
    /// thread of its own; `done` runs once both are done.
    fn discard(&self, fork: ForkTrace<'_>, done: Box<dyn FnOnce() + Send>);

    /// Whether the input box of the agent of `pane` holds no text, as the agent's screen
    /// shows it.
    fn input_is_empty(&self, pane: &PaneId) -> Result<bool, String>;

    /// Switches the agent of `switch.pane` to the session of the fork `switch.fork`, on a
    /// thread of its own. Once the fork's submit has its answer, or its process ended, it stops
    /// the fork's process and keeps its transcript; then, with the agent idle and its input box
    /// empty, it has the agent resume the fork's session, and waits until the agent's hook says
    /// that it did, and Herdr reports the agent on that session and ready for a prompt. `done`
    /// gets the agent as Herdr then reports it, or why the switch failed.
    fn switch(
        &self,
        switch: SwitchTo<'_>,
        done: Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>,
    );

    /// Has the agent of `pane`, idle with an empty input box, resume the session `session`, on
    /// a thread of its own, and waits until the agent's hook says that it did, and Herdr reports
    /// the agent on that session and ready for a prompt, as [`ForkHost::switch`] does once the
    /// fork stopped: it puts the agent back on the session its forks were taken from. `done`
    /// gets the agent as Herdr then reports it, or why it was not put back.
    fn resume(
        &self,
        pane: &PaneId,
        session: &str,
        done: Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>,
    );

    /// Adds `line` to run-ahead's log, which says what the forks did.
    fn log(&self, line: &str);
}
