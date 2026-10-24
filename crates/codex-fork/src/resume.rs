//! Moving the Codex agent of a Herdr pane to another session: the reviewer types Codex's own
//! `/resume <session>`, the daemon tells that a client joined the session and that it is idle,
//! then the host names the session as the pane's and reports it to Herdr.
//!
//! Text left in Codex's input box would join the typed `/resume` into one prompt, which the
//! agent would take a turn on, on the session it runs: while the agent moves, the reviewer's
//! Codex plugin blocks every prompt of that session, and the move fails.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agent_hooks::{AgentHooks, Expectation, Heard, HookedAgent};
use codex_daemon::{Daemon, SessionEvent, SessionStatus};
use herdr_client::client::HerdrClient;
use herdr_client::protocol::{Agent, AgentPort, PaneId, SessionReport};
use review_run_ahead::{RunAheadLog, SwitchFailure, resume_block};

/// How long the daemon's client waits to join the session, a request of the daemon's.
const JOIN_LIMIT: Duration = Duration::from_secs(10);

/// The source of Herdr's own Codex hook, whose reports the host's report takes the place of
/// until that hook reports the session itself, at the agent's next turn.
const HERDR_CODEX: &str = "herdr:codex";

/// The move of the Codex agent of `pane` to the session `session` of `daemon`.
pub(crate) struct Resume<'a> {
    pub(crate) herdr: &'a HerdrClient,
    pub(crate) pane: &'a PaneId,
    pub(crate) hooks: Option<&'a AgentHooks>,
    pub(crate) daemon: Daemon,
    pub(crate) session: &'a str,
    /// Another session the agent may run, besides the one the reviewer knows it on: a fork's,
    /// when the agent moves back from it after a switch that failed once `/resume` was typed.
    pub(crate) maybe_on: Option<&'a str>,
    /// How long it waits once the agent was told to resume the session.
    pub(crate) limit: Duration,
    /// How often it looks at what the plugin's hook and the daemon said meanwhile.
    pub(crate) poll: Duration,
    /// How long the daemon's client waits before it connects again after it lost the daemon.
    pub(crate) retry: Duration,
    pub(crate) log: &'a RunAheadLog,
}

/// What the daemon said of the session the agent moves to, as far as the move needs it.
#[derive(Default)]
struct Joined {
    /// Another client joined the session: the agent's TUI.
    resumed: bool,
    status: Option<SessionStatus>,
}

impl Joined {
    /// Takes what the daemon said since, from `events`.
    fn hear(&mut self, events: &Receiver<SessionEvent>) {
        for event in events.try_iter() {
            match event {
                SessionEvent::Resumed => self.resumed = true,
                SessionEvent::Joined(status) => self.status = status.or(self.status),
                SessionEvent::Status(status) => self.status = Some(status),
                SessionEvent::TurnStarted { .. } => self.status = Some(SessionStatus::Active),
                _ => {}
            }
        }
    }
}

impl Resume<'_> {
    /// Has the agent, idle, resume the session, and waits until the daemon tells that it did
    /// and that the session is idle: the agent as Herdr then reports it, or why not. A prompt
    /// submitted meanwhile on the session the agent ran is blocked, and fails the move.
    pub(crate) fn run(self) -> Result<Agent, SwitchFailure> {
        let hooks = self.hooks.ok_or_else(|| {
            SwitchFailure::untyped("the reviewer does not listen to the agent's hooks")
        })?;
        let agent = self
            .herdr
            .get_agent(self.pane)
            .map_err(|error| SwitchFailure::untyped(error.to_string()))?
            .ok_or_else(|| SwitchFailure::untyped("the agent's pane is gone"))?;
        if !agent.agent_status.waits_for_prompt() {
            return Err(SwitchFailure::untyped("the agent in the pane is working"));
        }
        let Some(hooked) = hooks.hooked(self.herdr, &agent) else {
            return Err(SwitchFailure::untyped(
                "the agent in the pane runs without the reviewer's Codex plugin",
            ));
        };
        let (heard, events) = mpsc::channel();
        let _follow = self.daemon.follow(self.session, self.retry, move |event| {
            let _ = heard.send(event);
        });
        // The client joins the session first, so that it hears the TUI join it next.
        let joined = self.join(&events, Instant::now() + JOIN_LIMIT)?;
        let command = format!("/resume {}", self.session);
        let block = resume_block(&command);
        // The prompt the text and the command make runs on the session the agent is on.
        let expectations: Vec<Expectation> = std::iter::once(hooked)
            .chain(self.maybe_on.map(|session| HookedAgent::Codex {
                session: session.to_owned(),
            }))
            .map(|on| hooks.expect_resume(&on, self.session, &block))
            .collect();
        self.herdr
            .submit_prompt(self.pane, &command)
            .map_err(|error| SwitchFailure::untyped(error.to_string()))?;
        let deadline = Instant::now() + self.limit;
        self.wait_until_moved(&expectations, &events, joined, &command, deadline)?;
        drop(expectations);
        hooks.name_session(self.pane, self.session);
        if agent.agent_session.is_some() {
            self.report_to_herdr();
        }
        Ok(self
            .herdr
            .get_agent(self.pane)
            .ok()
            .flatten()
            .unwrap_or(agent))
    }

    /// Waits, until `deadline`, for the daemon's client to join the session, as `events` tell;
    /// returns the session's status then.
    fn join(
        &self,
        events: &Receiver<SessionEvent>,
        deadline: Instant,
    ) -> Result<Joined, SwitchFailure> {
        loop {
            match events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(SessionEvent::Joined(status)) => {
                    return Ok(Joined {
                        resumed: false,
                        status,
                    });
                }
                Ok(SessionEvent::Refused(error)) => {
                    return Err(SwitchFailure::untyped(format!(
                        "the Codex daemon cannot join the session {}: {error}",
                        self.session
                    )));
                }
                Ok(_) => {}
                Err(_) => {
                    return Err(SwitchFailure::untyped(format!(
                        "the reviewer could not join the session {} on the Codex daemon",
                        self.session
                    )));
                }
            }
        }
    }

    /// Waits, until `deadline`, for the daemon to tell, through `events`, that another client
    /// joined the session and that it is idle, once the agent was told `command`; a prompt the
    /// plugin's hook blocked meanwhile, as `expectations` hear, fails the move.
    fn wait_until_moved(
        &self,
        expectations: &[Expectation],
        events: &Receiver<SessionEvent>,
        mut joined: Joined,
        command: &str,
        deadline: Instant,
    ) -> Result<(), SwitchFailure> {
        // The plugin's hook and the daemon wake no common waiter: the wait alternates.
        let slice = self.poll / u32::try_from(expectations.len()).unwrap_or(1).max(1);
        // What the hook blocked, waiting at most `limit` on each expectation.
        let blocked = |limit: Duration| {
            expectations
                .iter()
                .find_map(|expectation| match expectation.wait(limit) {
                    Some(Heard::Blocked { prompt }) => {
                        Some(SwitchFailure::blocked(&prompt, command))
                    }
                    _ => None,
                })
        };
        loop {
            if let Some(failure) = blocked(Duration::ZERO) {
                return Err(failure);
            }
            joined.hear(events);
            if joined.resumed && joined.status == Some(SessionStatus::Idle) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(SwitchFailure {
                    error: format!(
                        "the Codex daemon did not report the agent on the session {} within {} s",
                        self.session,
                        self.limit.as_secs_f64()
                    ),
                    typed: true,
                });
            }
            if let Some(failure) = blocked(slice) {
                return Err(failure);
            }
        }
    }

    /// Reports the session to Herdr, as Herdr's own Codex hook will at the agent's next turn:
    /// until then, Herdr would report the agent on the session it left, which the pinned agent
    /// and the access identity bind.
    fn report_to_herdr(&self) {
        let seq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |now| u64::try_from(now.as_nanos()).unwrap_or(u64::MAX));
        let reported = self.herdr.report_agent_session(
            self.pane,
            &SessionReport {
                source: HERDR_CODEX,
                agent: "codex",
                seq,
                session: self.session,
                start: "resume",
            },
        );
        if let Err(error) = reported {
            self.log.write(&format!(
                "Herdr was not told that the agent of {} runs the session {}: {error}",
                self.pane.0, self.session
            ));
        }
    }
}
