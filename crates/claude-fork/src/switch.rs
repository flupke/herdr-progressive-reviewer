//! Switching the Claude Code agent of a Herdr pane to a fork's session: the fork stops once
//! its submit has its answer, then the agent resumes the fork's session with Claude Code's own
//! `/resume`, and Herdr reports it on that session. The same `/resume` puts the agent back on
//! the session its forks were taken from.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use agent_fork::{ProcessStamp, RunningFork, stop_recorded};
use herdr_client::client::HerdrClient;
use herdr_client::protocol::{Agent, AgentPort, PaneId};
use review_run_ahead::SwitchFailure;

use crate::screen::input_box_text;
use crate::stream::Signal;

/// How long a switch waits for the answer to the fork's submit to reach the fork, which then
/// writes it in its transcript, before it stops the fork anyway.
const ANSWER_WAIT: Duration = Duration::from_secs(10);
/// How long a switch waits for Herdr to report the agent on the fork's session, ready for a
/// prompt, once the agent was told to resume it.
const RESUME_WAIT: Duration = Duration::from_secs(20);
/// How often a switch asks Herdr where the agent stands while it waits: Herdr sends no event
/// when an agent's session changes.
const RESUME_POLL: Duration = Duration::from_millis(100);

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

    /// Waits, at most [`ANSWER_WAIT`], until the answer to the fork's submit reached it.
    fn wait_for_answer(&self) {
        if let Self::Live(live) = self {
            live.answered.wait(ANSWER_WAIT);
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

    /// The agent, once it waits for a prompt on the session `session` with an empty input box.
    fn ready_on(&self, session: &str) -> Option<Agent> {
        self.agent().ok().filter(|agent| {
            agent.agent_status.waits_for_prompt()
                && agent
                    .agent_session
                    .as_ref()
                    .is_some_and(|reported| reported.value == session)
                && self.input_box_empty() == Ok(Some(true))
        })
    }
}

/// The switch of the agent of `pane` to the session `session` of the fork `fork`.
pub(crate) struct Switch<'a> {
    pub(crate) pane: AgentPane<'a>,
    pub(crate) session: &'a str,
    pub(crate) fork: ForkProcess,
}

impl Switch<'_> {
    /// Runs the switch, as [`review_run_ahead::ForkHost::switch`] describes it: the agent as
    /// Herdr then reports it, or why the switch failed.
    pub(crate) fn run(self) -> Result<Agent, SwitchFailure> {
        // From here on, only the agent writes the fork's transcript, which ends with the
        // answer to its submit.
        self.fork.wait_for_answer();
        self.fork.stop(self.session);
        Resume {
            pane: self.pane,
            session: self.session,
        }
        .run()
    }
}

/// The agent of `pane` resuming the session `session` with Claude Code's own `/resume`.
pub(crate) struct Resume<'a> {
    pub(crate) pane: AgentPane<'a>,
    pub(crate) session: &'a str,
}

impl Resume<'_> {
    /// Has the agent, idle with an empty input box, resume the session, then waits until
    /// Herdr reports it there, idle with an empty input box: the agent as Herdr then reports
    /// it, or why not. Claude Code takes what is typed in its pane in order, so once the
    /// command is typed, the agent ends on this session even after an earlier `/resume`.
    pub(crate) fn run(self) -> Result<Agent, SwitchFailure> {
        let failed = |error: String| SwitchFailure {
            error,
            typed: false,
        };
        let agent = self.pane.agent().map_err(failed)?;
        if !agent.agent_status.waits_for_prompt() {
            return Err(failed("the agent in the pane is working".into()));
        }
        if self.pane.input_box_empty().map_err(failed)? != Some(true) {
            return Err(failed("text waits in the agent's input box".into()));
        }
        self.pane
            .herdr
            .submit_agent_command(self.pane.pane, &format!("/resume {}", self.session))
            .map_err(|error| failed(error.to_string()))?;
        let deadline = Instant::now() + RESUME_WAIT;
        loop {
            if let Some(agent) = self.pane.ready_on(self.session) {
                return Ok(agent);
            }
            if Instant::now() >= deadline {
                return Err(SwitchFailure {
                    error: format!(
                        "Herdr did not report the agent on the session {}, ready for a \
                         prompt, within {} s",
                        self.session,
                        RESUME_WAIT.as_secs()
                    ),
                    typed: true,
                });
            }
            thread::sleep(RESUME_POLL);
        }
    }
}
