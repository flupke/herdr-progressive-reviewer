//! Switching the Claude Code agent of a Herdr pane to a fork's session: the fork stops once
//! its submit has its answer, then the agent resumes the fork's session with Claude Code's own
//! `/resume`, its hook says that it did, and Herdr reports it on that session. The same
//! `/resume` puts the agent back on the session its forks were taken from.
//!
//! Text left in the agent's input box would join the typed `/resume` into one prompt, which the
//! agent would take a turn on. A command of Claude Code's own submits no prompt, so while the
//! agent resumes the session, the reviewer blocks every prompt submitted to it, through the
//! agent's `UserPromptSubmit` hook: the switch then fails, and the answer waits for Retry.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use agent_fork::{ProcessStamp, RunningFork, stop_recorded};
use agent_hooks::{AgentHooks, Heard};
use herdr_client::client::HerdrClient;
use herdr_client::protocol::{Agent, AgentPort, PaneId};
use review_run_ahead::SwitchFailure;

use crate::host::ForkWaits;
use crate::stream::Signal;

/// A fork that runs, and the signal of the answer to its submit.
pub(crate) struct LiveFork {
    pub(crate) process: Arc<RunningFork>,
    pub(crate) answered: Arc<Signal>,
}

/// The process of a fork, as far as the host knows it.
pub(crate) enum ForkProcess {
    /// A fork the host started and follows.
    Live(LiveFork),
    /// A fork whose process only its record names, such as one a stopped reviewer started.
    Recorded(ProcessStamp),
    /// A fork that never started.
    Unknown,
}

impl ForkProcess {
    /// The fork the host follows, `live`, or else the one recorded as `process`.
    pub(crate) fn of(live: Option<LiveFork>, process: Option<ProcessStamp>) -> Self {
        match (live, process) {
            (Some(live), _) => Self::Live(live),
            (None, Some(process)) => Self::Recorded(process),
            (None, None) => Self::Unknown,
        }
    }

    /// Stops the fork of the session `session` if its process still runs.
    pub(crate) fn stop(&self, session: &str) {
        match self {
            Self::Live(live) => live.process.terminate(),
            Self::Recorded(process) => {
                stop_recorded(*process, session);
            }
            Self::Unknown => {}
        }
    }

    /// Waits, at most `waits.answer`, until the answer to the fork's submit reached it.
    fn wait_for_answer(&self, waits: ForkWaits) {
        if let Self::Live(live) = self {
            live.answered.wait(waits.answer);
        }
    }
}

/// The Claude Code agent of a Herdr pane.
pub(crate) struct AgentPane<'a> {
    pub(crate) herdr: &'a HerdrClient,
    pub(crate) pane: &'a PaneId,
}

impl AgentPane<'_> {
    /// The agent, as Herdr reports it now.
    fn agent(&self) -> Result<Agent, String> {
        self.herdr
            .get_agent(self.pane)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "the agent's pane is gone".to_owned())
    }

    /// Whether `agent`, the agent of this pane, runs the hooks of the reviewer's plugin: its
    /// process, still in the pane, started the session Herdr reports with them, as `hooks`
    /// recorded it.
    pub(crate) fn hooked(&self, hooks: &AgentHooks, agent: &Agent) -> bool {
        let Some(session) = &agent.agent_session else {
            return false;
        };
        let started: Vec<_> = hooks
            .hooked_sessions(self.pane)
            .into_iter()
            .filter(|started| started.session == session.value && started.process.is_running())
            .collect();
        !started.is_empty()
            && self
                .herdr
                .pane_process_info(self.pane)
                .is_ok_and(|processes| {
                    processes.foreground_processes.iter().any(|process| {
                        started
                            .iter()
                            .any(|started| started.process.pid == process.pid)
                    })
                })
    }

    /// The agent, once Herdr reports it waiting for a prompt on the session `session`.
    fn ready_on(&self, session: &str) -> Option<Agent> {
        self.agent().ok().filter(|agent| {
            agent.agent_status.waits_for_prompt()
                && agent
                    .agent_session
                    .as_ref()
                    .is_some_and(|reported| reported.value == session)
        })
    }
}

/// The switch of the agent of `pane` to the session `session` of the fork `fork`.
pub(crate) struct Switch<'a> {
    pub(crate) pane: AgentPane<'a>,
    pub(crate) session: &'a str,
    pub(crate) fork: ForkProcess,
    pub(crate) waits: ForkWaits,
    pub(crate) hooks: Option<&'a AgentHooks>,
}

impl Switch<'_> {
    /// Runs the switch, as [`review_run_ahead::ForkHost::switch`] describes it: the agent as
    /// Herdr then reports it, or why the switch failed.
    pub(crate) fn run(self) -> Result<Agent, SwitchFailure> {
        // From here on, only the agent writes the fork's transcript, which ends with the
        // answer to its submit.
        self.fork.wait_for_answer(self.waits);
        self.fork.stop(self.session);
        Resume {
            pane: self.pane,
            session: self.session,
            limit: self.waits.resume,
            poll: self.waits.resume_poll,
            hooks: self.hooks,
        }
        .run()
    }
}

/// The agent of `pane` resuming the session `session` with Claude Code's own `/resume`, which
/// the agent's hooks tell `hooks`.
pub(crate) struct Resume<'a> {
    pub(crate) pane: AgentPane<'a>,
    pub(crate) session: &'a str,
    /// How long it waits, once the agent was told to resume the session.
    pub(crate) limit: Duration,
    /// How often it asks Herdr where the agent stands, once the agent's hook said that it
    /// resumed the session.
    pub(crate) poll: Duration,
    pub(crate) hooks: Option<&'a AgentHooks>,
}

impl Resume<'_> {
    /// Has the agent, idle, resume the session, then waits until its hook says that it resumed
    /// it, and Herdr reports it there, idle: the agent as Herdr then reports it, or why not. A
    /// prompt submitted meanwhile is blocked, and fails the move. Claude Code takes what is
    /// typed in its pane in order, so once the command is typed, the agent ends on this session
    /// even after an earlier `/resume`.
    pub(crate) fn run(self) -> Result<Agent, SwitchFailure> {
        let failed = |error: String| SwitchFailure {
            error,
            typed: false,
        };
        let hooks = self
            .hooks
            .ok_or_else(|| failed("the reviewer does not listen to the agent's hooks".into()))?;
        let agent = self.pane.agent().map_err(failed)?;
        if !agent.agent_status.waits_for_prompt() {
            return Err(failed("the agent in the pane is working".into()));
        }
        if !self.pane.hooked(hooks, &agent) {
            return Err(failed(
                "the agent in the pane runs without the reviewer's Claude Code plugin".into(),
            ));
        }
        let command = format!("/resume {}", self.session);
        let deadline = Instant::now() + self.limit;
        let block = format!(
            "The progressive reviewer stopped this prompt: it typed `{command}` while text \
             waited in the input box, and both would have run as one prompt. Your text is \
             below; the reviewer's answer waits for Retry."
        );
        let expectation = hooks.expect_resume(self.pane.pane, self.session, &block);
        self.pane
            .herdr
            .submit_agent_command(self.pane.pane, &command)
            .map_err(|error| failed(error.to_string()))?;
        let late = |what: &str| SwitchFailure {
            error: format!(
                "{what} the agent on the session {} within {} s",
                self.session,
                self.limit.as_secs_f64()
            ),
            typed: true,
        };
        match expectation.wait(deadline.saturating_duration_since(Instant::now())) {
            Some(Heard::Resumed) => {}
            // The text and the command made one prompt, which never ran: the agent did not
            // move.
            Some(Heard::Blocked { prompt }) if prompt.contains(&command) => {
                return Err(failed(
                    "text in the agent's input box joined the /resume, so the reviewer stopped \
                     both; the agent's pane shows the text"
                        .into(),
                ));
            }
            // A prompt of someone else: the command may still run.
            Some(Heard::Blocked { .. }) => {
                return Err(SwitchFailure {
                    error: "a prompt reached the agent while it resumed the session, and the \
                            reviewer stopped it"
                        .into(),
                    typed: true,
                });
            }
            None => return Err(late("the agent's hook did not report")),
        }
        drop(expectation);
        // Herdr hears of the session from its own hook, which Claude Code runs beside the
        // reviewer's: Herdr may not know it yet, and sends no event once it does.
        loop {
            if let Some(agent) = self.pane.ready_on(self.session) {
                return Ok(agent);
            }
            if Instant::now() >= deadline {
                return Err(late("Herdr did not report"));
            }
            thread::sleep(self.poll);
        }
    }
}
