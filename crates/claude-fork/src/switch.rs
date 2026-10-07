//! Switching the Claude Code agent of a Herdr pane to a fork's session: the fork stops once
//! its submit has its answer, then the agent resumes the fork's session with Claude Code's own
//! `/resume`, its hook says that it did, and Herdr reports it on that session. The same
//! `/resume` puts the agent back on the session its forks were taken from.

use std::sync::Arc;
use std::thread;
use std::time::Instant;

use agent_fork::{ProcessStamp, RunningFork, stop_recorded};
use agent_hooks::{AgentHooks, Heard};
use herdr_client::client::HerdrClient;
use herdr_client::protocol::{Agent, AgentPort, PaneId};
use review_run_ahead::SwitchFailure;

use crate::host::ForkWaits;
use crate::screen::input_box_text;
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
    /// Whether the agent's screen shows its input box empty; `None` when it shows no input box.
    pub(crate) fn input_box_empty(&self) -> Result<Option<bool>, String> {
        let screen = self
            .herdr
            .read_agent_screen_styled(self.pane)
            .map_err(|error| error.to_string())?;
        Ok(input_box_text(&screen).map(|text| text.is_empty()))
    }

    /// The agent, as Herdr reports it now.
    fn agent(&self) -> Result<Agent, String> {
        self.herdr
            .get_agent(self.pane)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "the agent's pane is gone".to_owned())
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
            waits: self.waits,
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
    pub(crate) waits: ForkWaits,
    pub(crate) hooks: Option<&'a AgentHooks>,
}

impl Resume<'_> {
    /// Has the agent, idle with an empty input box, resume the session, then waits until its
    /// hook says that it resumed it, and Herdr reports it there, idle: the agent as Herdr then
    /// reports it, or why not. Claude Code takes what is typed in its pane in order, so once the
    /// command is typed, the agent ends on this session even after an earlier `/resume`.
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
        if self.pane.input_box_empty().map_err(failed)? != Some(true) {
            return Err(failed("text waits in the agent's input box".into()));
        }
        let deadline = Instant::now() + self.waits.resume;
        let expectation = hooks.expect_resume(self.pane.pane, self.session);
        self.pane
            .herdr
            .submit_agent_command(self.pane.pane, &format!("/resume {}", self.session))
            .map_err(|error| failed(error.to_string()))?;
        let late = |what: &str| SwitchFailure {
            error: format!(
                "{what} the agent on the session {} within {} s",
                self.session,
                self.waits.resume.as_secs_f64()
            ),
            typed: true,
        };
        match expectation.wait(deadline.saturating_duration_since(Instant::now())) {
            Some(Heard::Resumed) => {}
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
            thread::sleep(self.waits.resume_poll);
        }
    }
}
