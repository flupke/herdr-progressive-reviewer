//! [`CodexForks`]: run-ahead's forks of a Codex agent in a Herdr pane, on the agent's daemon.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use agent_hooks::AgentHooks;
use codex_daemon::{Daemon, NoDaemon, Rpc};
use herdr_client::client::HerdrClient;
use herdr_client::protocol::{Agent, AgentPort, PaneId};
use review_run_ahead::{
    ForkEnd, ForkHost, ForkPoint, ForkStart, ForkTrace, ForkWaits, HostFields, OwnedFork,
    PaneWatch, RunAheadLog, StatusReport, SwitchFailure, SwitchTo,
};
use serde_json::{Value, json};

use crate::fields;
use crate::live::{LiveFork, TurnWatch};
use crate::resume::Resume;

/// How long the host waits for the daemon's answer to a request.
const REQUEST_LIMIT: Duration = Duration::from_secs(10);

/// Forks of a Codex agent's session, run by the agent's daemon and followed over a connection
/// of their own until they are discarded. A fork lives in the daemon, not in the reviewer's
/// process: the reviewer's session discards those that run when it closes, and a reopened
/// reviewer those a stopped one left. Dropping the host lets the discards under way finish.
pub struct CodexForks {
    herdr: HerdrClient,
    /// What the agents' hooks tell the reviewer, when it listens to them: the session a Codex
    /// agent named last, whether its session runs the reviewer's plugin, and the forks' marks
    /// its guard reads.
    hooks: Option<Arc<AgentHooks>>,
    log: RunAheadLog,
    waits: ForkWaits,
    /// The forks that run, or ran, by session.
    live: Mutex<HashMap<String, LiveFork>>,
    /// The discards and switches under way.
    threads: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl CodexForks {
    /// Forks of the Codex agents `herdr` reports, logged to `log`, waiting on Herdr, the daemon
    /// and the forks as `waits` say, and on what the agents' hooks tell the reviewer through
    /// `hooks`, when it listens to them.
    pub fn new(
        herdr: HerdrClient,
        log: RunAheadLog,
        waits: ForkWaits,
        hooks: Option<Arc<AgentHooks>>,
    ) -> Self {
        Self {
            herdr,
            hooks,
            log,
            waits,
            live: Mutex::default(),
            threads: Mutex::default(),
        }
    }

    fn live(&self) -> MutexGuard<'_, HashMap<String, LiveFork>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What moves the agent between sessions, on a thread of its own.
    fn mover(&self) -> Mover {
        Mover {
            herdr: self.herdr.clone(),
            hooks: self.hooks.clone(),
            log: self.log.clone(),
            waits: self.waits,
        }
    }

    /// Runs `work` on a thread of its own, which a dropped host waits for.
    fn in_background(&self, work: impl FnOnce() + Send + 'static) {
        let mut threads = self.threads.lock().unwrap_or_else(PoisonError::into_inner);
        threads.retain(|thread| !thread.is_finished());
        threads.push(thread::spawn(work));
    }

    /// The daemon of `agent`, as its process in its pane says.
    fn daemon_of(&self, agent: &Agent) -> Result<Daemon, NoDaemon> {
        let processes = self
            .herdr
            .pane_process_info(&agent.pane_id)
            .map_err(|_| NoDaemon::Unreadable("pane"))?;
        Daemon::of_pane(&processes.foreground_processes)
    }

    /// Whether the reviewer's Codex plugin has its fork guard installed and trusted, so that a
    /// new session of `daemon`, a fork, runs it.
    fn guarded(daemon: &Daemon) -> Result<bool, String> {
        let rpc = connect(daemon)?;
        let listed = rpc.request("hooks/list", &json!({}), REQUEST_LIMIT)?;
        let hooks = listed
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|directory| directory.get("hooks").and_then(Value::as_array))
            .flatten();
        Ok(hooks.into_iter().any(|hook| {
            let field = |name: &str| hook.get(name).and_then(Value::as_str);
            field("pluginId") == Some(codex_hooks::PLUGIN)
                && field("eventName") == Some("preToolUse")
                && hook.get("enabled").and_then(Value::as_bool) == Some(true)
                && matches!(field("trustStatus"), Some("trusted" | "managed"))
        }))
    }

    /// The last turn of `session` on the daemon `rpc` reaches.
    fn last_turn(rpc: &Rpc, session: &str) -> Result<Option<String>, String> {
        Ok(latest_turn(rpc, session)?
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned))
    }
}

impl Drop for CodexForks {
    /// Lets the discards under way finish, so that no discarded fork is left on the daemon.
    fn drop(&mut self) {
        let threads =
            std::mem::take(&mut *self.threads.lock().unwrap_or_else(PoisonError::into_inner));
        for thread in threads {
            let _ = thread.join();
        }
    }
}

/// A connection to `daemon` that hears nothing but the answers to its requests.
fn connect(daemon: &Daemon) -> Result<Rpc, String> {
    daemon.connect(|_, _| {})
}

/// Interrupts the turn `turn` of the session `session`, over `rpc`. A turn that ended refuses
/// it, which changes nothing.
fn interrupt(rpc: &Rpc, session: &str, turn: &str) {
    let _ = rpc.request(
        "turn/interrupt",
        &json!({"threadId": session, "turnId": turn}),
        REQUEST_LIMIT,
    );
}

/// Gives the session `session` the settings `settings`, as `thread/settings/update` takes them,
/// over `rpc`.
fn update_settings(rpc: &Rpc, session: &str, settings: &Value) -> Result<(), String> {
    let mut params = settings.clone();
    params["threadId"] = session.into();
    rpc.request("thread/settings/update", &params, REQUEST_LIMIT)
        .map(|_| ())
}

/// Ends the turn of the fork `fork` of `daemon`, followed over `live` when the host started it:
/// waits `waits.answer` for it to end by itself, as it does once its submit has its answer, then
/// interrupts it and waits as long again. Returns whether it ended. The fork's connection, its
/// only client, closes: the agent's TUI is its session's client from then on, and answers its
/// approvals.
fn end_turn(live: Option<LiveFork>, daemon: &Daemon, fork: &OwnedFork, waits: ForkWaits) -> bool {
    let limit = waits.answer;
    let Some(live) = live else {
        // A fork a stopped reviewer left: no client follows its turn, so the daemon is asked
        // where it stands, every `resume_poll`, which no event tells this connection.
        let Ok(rpc) = connect(daemon) else {
            return false;
        };
        if let Some(turn) = fields::turn(&fork.host) {
            interrupt(&rpc, &fork.session, &turn);
        }
        let deadline = std::time::Instant::now() + limit;
        loop {
            if last_turn_ended(&rpc, &fork.session) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            thread::sleep(waits.resume_poll);
        }
    };
    live.turn.wait_over(limit) || {
        if let Some(turn) = live.turn.id() {
            interrupt(&live.rpc, &fork.session, &turn);
        }
        live.turn.wait_over(limit)
    }
}

/// The last turn of `session`, over `rpc`, as `thread/turns/list` gives it; null when it has
/// none.
fn latest_turn(rpc: &Rpc, session: &str) -> Result<Value, String> {
    let mut turns = rpc.request(
        "thread/turns/list",
        &json!({"threadId": session, "limit": 1, "sortDirection": "desc"}),
        REQUEST_LIMIT,
    )?;
    Ok(turns
        .pointer_mut("/data/0")
        .map(Value::take)
        .unwrap_or_default())
}

/// Whether the last turn of `session`, over `rpc`, ended, as the daemon says.
fn last_turn_ended(rpc: &Rpc, session: &str) -> bool {
    latest_turn(rpc, session).is_ok_and(|turn| {
        turn.get("status")
            .and_then(Value::as_str)
            .is_none_or(|status| status != "inProgress")
    })
}

/// Why a fork cannot be moved to: its record names no daemon.
fn no_daemon() -> SwitchFailure {
    SwitchFailure::untyped("the fork's record names no Codex daemon")
}

/// What moves the agent of a pane between sessions, on a thread of its own.
struct Mover {
    herdr: HerdrClient,
    hooks: Option<Arc<AgentHooks>>,
    log: RunAheadLog,
    waits: ForkWaits,
}

impl Mover {
    /// Makes the session of the fork `fork`, on `daemon`, one the agent may run: the guard no
    /// longer treats it as a fork's, and it takes back the agent's settings, so that the agent's
    /// own tools run as they did. Logs what failed.
    fn hand_over(&self, daemon: &Daemon, fork: ForkTrace<'_>) {
        let session = fork.session;
        if let Some(hooks) = &self.hooks
            && let Err(error) = hooks.root().unmark_codex_fork(session)
        {
            self.log.write(&format!(
                "the fork {session}'s guard was not lifted: {error}"
            ));
        }
        let given_back = fields::settings(fork.host)
            .ok_or_else(|| "its record keeps no settings of the agent's".to_owned())
            .and_then(|settings| update_settings(&connect(daemon)?, session, &settings));
        if let Err(error) = given_back {
            self.log.write(&format!(
                "the fork {session}'s session did not take back the agent's settings: {error}"
            ));
        }
    }

    /// Has the agent of `pane` resume `session` of `daemon`, waiting `limit` for it, as
    /// [`Resume`] says.
    fn resume(
        &self,
        pane: &PaneId,
        daemon: Daemon,
        session: &str,
        maybe_on: Option<&str>,
        limit: Duration,
    ) -> Result<Agent, SwitchFailure> {
        Resume {
            herdr: &self.herdr,
            pane,
            hooks: self.hooks.as_deref(),
            daemon,
            session,
            maybe_on,
            limit,
            poll: self.waits.resume_poll,
            retry: self.waits.resubscribe,
            log: &self.log,
        }
        .run()
    }
}

/// Interrupts the turn `turn` of the fork `session`, if it still runs, then deletes the fork,
/// over `rpc`. Returns what failed, if anything.
fn stop_and_delete(rpc: &Rpc, session: &str, turn: Option<&str>) -> Result<(), String> {
    if let Some(turn) = turn {
        interrupt(rpc, session, turn);
    }
    rpc.request(
        "thread/delete",
        &json!({"threadId": session}),
        REQUEST_LIMIT,
    )
    .map(|_| ())
}

impl ForkHost for CodexForks {
    fn watch(&self, pane: &PaneId, report: StatusReport) -> PaneWatch {
        PaneWatch::herdr(&self.herdr, pane, self.waits.resubscribe, report)
    }

    fn session(&self, agent: &Agent) -> Option<String> {
        self.hooks.as_ref()?.named_session(&agent.pane_id)
    }

    fn unhooked(&self, agent: &Agent) -> Option<String> {
        // An agent run-ahead cannot fork for another reason has that reason.
        let hooks = self.hooks.as_ref()?;
        if matches!(
            self.daemon_of(agent),
            Err(NoDaemon::WithoutDaemon | NoDaemon::Remote(_))
        ) {
            return None;
        }
        // The session is unknown until the agent calls the reviewer's tools, as after the
        // reviewer reopened: the agent may still run the hooks.
        hooks.named_session(&agent.pane_id)?;
        hooks.hooked(&self.herdr, agent).is_none().then(|| {
            "the Codex agent's session runs without the reviewer's Codex plugin: install it \
             with `make install`, then restart the agent"
                .into()
        })
    }

    fn can_move(&self, agent: &Agent) -> bool {
        self.hooks
            .as_ref()
            .is_some_and(|hooks| hooks.hooked(&self.herdr, agent).is_some())
            && self.daemon_of(agent).is_ok()
    }

    fn unforkable(&self, agent: &Agent) -> Option<String> {
        if self.hooks.is_none() {
            return Some("the reviewer hears no agent's hooks: its log says why".into());
        }
        let daemon = match self.daemon_of(agent) {
            Ok(daemon) => daemon,
            Err(error @ (NoDaemon::WithoutDaemon | NoDaemon::Remote(_))) => {
                return Some(error.to_string());
            }
            // The agent may be starting: `point` says why it cannot be forked yet.
            Err(_) => return None,
        };
        match Self::guarded(&daemon) {
            Ok(true) => None,
            Err(error) => Some(error),
            Ok(false) => Some(
                "the reviewer's Codex plugin has no guard for run-ahead's forks: run `make \
                 install`"
                    .into(),
            ),
        }
    }

    fn point(&self, agent: &Agent) -> Result<ForkPoint, String> {
        let session = self
            .session(agent)
            .ok_or("the Codex agent named no session in a call to the reviewer's tools")?;
        let daemon = self.daemon_of(agent).map_err(|error| error.to_string())?;
        let rpc = connect(&daemon)?;
        let entry = Self::last_turn(&rpc, &session)?;
        Ok(ForkPoint {
            session,
            entry,
            model: None,
            host: fields::point(&daemon),
        })
    }

    fn last_entry(&self, point: &ForkPoint) -> Option<String> {
        let rpc = connect(&fields::daemon(&point.host)?).ok()?;
        Self::last_turn(&rpc, &point.session).ok().flatten()
    }

    fn create(&self, point: &ForkPoint) -> Result<OwnedFork, String> {
        let daemon = fields::daemon(&point.host).ok_or("the point names no Codex daemon")?;
        let hooks = self
            .hooks
            .as_ref()
            .ok_or("the reviewer hears no agent's hooks")?;
        let turn = Arc::new(TurnWatch::default());
        let rpc = daemon.connect(turn.handler(self.log.clone()))?;
        // A fork without overrides keeps the agent's settings, which its answer gives.
        let forked = rpc.request(
            "thread/fork",
            &json!({"threadId": point.session, "excludeTurns": true}),
            REQUEST_LIMIT,
        )?;
        let fork = forked
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or("the Codex daemon named no fork")?
            .to_owned();
        let agent_settings = json!({
            "approvalPolicy": forked.get("approvalPolicy"),
            "sandboxPolicy": forked.get("sandbox"),
            "approvalsReviewer": forked.get("approvalsReviewer"),
        });
        // Read-only, with every approval asked of the reviewer, the fork's only client. Codex
        // appends these permissions after the history at the next turn, which leaves the
        // agent's prompt cache.
        let guarded = hooks
            .root()
            .mark_codex_fork(&fork)
            .map_err(|error| format!("the fork's guard could not be armed: {error}"))
            .and_then(|()| {
                update_settings(
                    &rpc,
                    &fork,
                    &json!({
                        "approvalPolicy": "on-request",
                        "sandboxPolicy": {"type": "readOnly"},
                        "approvalsReviewer": "user",
                    }),
                )
            });
        if let Err(error) = guarded {
            let _ = stop_and_delete(&rpc, &fork, None);
            let _ = hooks.root().unmark_codex_fork(&fork);
            return Err(error);
        }
        turn.forked(&fork);
        self.live().insert(fork.clone(), LiveFork { rpc, turn });
        Ok(OwnedFork {
            session: fork,
            host: fields::fork(&daemon, &agent_settings),
        })
    }

    fn start(
        &self,
        start: ForkStart<'_>,
        prompt: String,
        ended: Box<dyn FnOnce(ForkEnd) + Send>,
    ) -> Result<HostFields, String> {
        let live = self.live();
        let fork = live
            .get(start.fork.session)
            .ok_or("the fork's connection to the Codex daemon is gone")?;
        fork.turn.starting(ended);
        let started = fork.rpc.request(
            "turn/start",
            &json!({"threadId": start.fork.session, "input": [{"type": "text", "text": prompt}]}),
            REQUEST_LIMIT,
        )?;
        let turn = started
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .ok_or("the Codex daemon named no turn")?;
        fork.turn.started(turn);
        Ok(fields::started(turn))
    }

    fn discard(&self, fork: ForkTrace<'_>, done: Box<dyn FnOnce() + Send>) {
        let live = self.live().remove(fork.session);
        let session = fork.session.to_owned();
        let recorded = (fields::daemon(fork.host), fields::turn(fork.host));
        let (hooks, log) = (self.hooks.clone(), self.log.clone());
        self.in_background(move || {
            let deleted = match live {
                Some(live) => stop_and_delete(&live.rpc, &session, live.turn.id().as_deref()),
                None => match recorded {
                    (Some(daemon), turn) => connect(&daemon)
                        .and_then(|rpc| stop_and_delete(&rpc, &session, turn.as_deref())),
                    (None, _) => Err("its record names no Codex daemon".into()),
                },
            };
            if let Err(error) = deleted {
                log.write(&format!(
                    "the Codex fork {session} was not deleted: {error}"
                ));
            }
            if let Some(hooks) = hooks {
                let _ = hooks.root().unmark_codex_fork(&session);
            }
            done();
        });
    }

    fn switch(
        &self,
        switch: SwitchTo<'_>,
        done: Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>,
    ) {
        let live = self.live().remove(switch.fork.session);
        let (fork, pane) = (OwnedFork::of(switch.fork), switch.pane.clone());
        let mover = self.mover();
        self.in_background(move || {
            let daemon = fields::daemon(&fork.host).ok_or_else(no_daemon);
            done(daemon.and_then(|daemon| {
                if !end_turn(live, &daemon, &fork, mover.waits) {
                    return Err(SwitchFailure::untyped(
                        "the fork's turn did not end once interrupted",
                    ));
                }
                mover.hand_over(&daemon, fork.trace());
                mover.resume(&pane, daemon, &fork.session, None, mover.waits.resume)
            }));
        });
    }

    fn resume(
        &self,
        pane: &PaneId,
        from: ForkTrace<'_>,
        session: &str,
        done: Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>,
    ) {
        let (pane, session, fork) = (pane.clone(), session.to_owned(), OwnedFork::of(from));
        let mover = self.mover();
        self.in_background(move || {
            let daemon = fields::daemon(&fork.host).ok_or_else(no_daemon);
            done(daemon.and_then(|daemon| {
                // The agent may run the fork's session, which a switch had it resume.
                let maybe_on = Some(fork.session.as_str());
                mover.resume(&pane, daemon, &session, maybe_on, mover.waits.settle)
            }));
        });
    }

    fn log(&self, line: &str) {
        self.log.write(line);
    }
}

#[cfg(test)]
#[path = "host.tests.rs"]
mod tests;
