//! [`ClaudeForks`]: run-ahead's forks of a Claude Code agent in a Herdr pane.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use agent_fork::{Launcher, Wrapper};
use agent_hooks::AgentHooks;
use herdr_client::client::HerdrClient;
use herdr_client::protocol::{Agent, AgentPort, PaneId, PaneProcess};
use review_run_ahead::{
    ForkEnd, ForkHost, ForkPoint, ForkStart, ForkTrace, ForkWaits, HostFields, OwnedFork,
    PaneWatch, RunAheadLog, StatusReport, SwitchFailure, SwitchTo,
};

use crate::fields;
use crate::guard::SUBMITS;
use crate::pane::PaneClaude;
use crate::stream::StreamTally;
use crate::switch::{AgentPane, ForkProcess, LiveFork, Resume, Switch};
use crate::transcripts::Transcripts;

/// The programs forks need from the reviewer's installation: `reviewer-control`, whose
/// `fork-exec` ties a fork's life to the reviewer's and whose `fork-guard` is the forks' hook.
#[derive(Clone, Debug)]
pub struct ForkTools {
    pub control: PathBuf,
}

impl ForkTools {
    /// The `reviewer-control` installed beside the running program.
    pub fn beside_current_exe() -> std::io::Result<Self> {
        Ok(Self {
            control: std::env::current_exe()?.with_file_name("reviewer-control"),
        })
    }

    /// What starts a process that must not outlive the reviewer: `fork-exec`.
    pub fn wrapper(&self) -> Wrapper {
        Wrapper {
            program: self.control.clone(),
            arguments: vec!["fork-exec".into()],
        }
    }

    /// The forks' settings: the guard as their `PreToolUse` hook.
    fn settings(&self) -> String {
        let control = self.control.to_string_lossy().replace('\'', r"'\''");
        serde_json::json!({
            "hooks": {"PreToolUse": [{"matcher": "*", "hooks": [
                {"type": "command", "command": format!("'{control}' fork-guard")}
            ]}]}
        })
        .to_string()
    }
}

/// Forks of a Claude Code agent's session, started from the [`Launcher`]'s thread and followed
/// until they end. Dropping it sends every fork still running its parent-death signal.
pub struct ClaudeForks {
    herdr: HerdrClient,
    tools: ForkTools,
    launcher: Launcher,
    log: RunAheadLog,
    waits: ForkWaits,
    /// What the hooks of the agents say, when the reviewer listens to them.
    hooks: Option<Arc<AgentHooks>>,
    /// The forks that run, by session.
    live: Arc<Mutex<HashMap<String, LiveFork>>>,
    /// The discards under way, which stop a fork and delete its transcript.
    discards: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl ClaudeForks {
    /// Forks of the agents `herdr` reports, started through `tools`, logged to `log`, waiting
    /// on Herdr and on the forks as `waits` say, and on what the agents' hooks tell the reviewer
    /// through `hooks`, or else why the reviewer does not listen to them.
    pub fn new(
        herdr: HerdrClient,
        tools: ForkTools,
        log: RunAheadLog,
        waits: ForkWaits,
        hooks: Result<Arc<AgentHooks>, String>,
    ) -> Self {
        let (hooks, unheard) = match hooks {
            Ok(hooks) => (Some(hooks), None),
            Err(error) => (None, Some(error)),
        };
        let forks = Self {
            launcher: Launcher::start(tools.wrapper()),
            herdr,
            tools,
            log,
            waits,
            hooks,
            live: Arc::default(),
            discards: Mutex::default(),
        };
        if let Some(error) = unheard {
            forks.log(&error);
        }
        forks
    }

    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<String, LiveFork>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ClaudeForks {
    /// The hooks the reviewer hears `agent` through, and its session, when run-ahead can fork
    /// the agent if it runs them; otherwise why not.
    fn forkable<'a>(&'a self, agent: &'a Agent) -> Result<(&'a AgentHooks, &'a str), String> {
        if !cfg!(target_os = "linux") {
            // Only Linux has the parent-death signal that keeps a fork from outliving the
            // reviewer when the reviewer runs outside a terminal.
            return Err("run-ahead forks only on Linux".into());
        }
        if !runs_claude(agent) {
            return Err(format!(
                "run-ahead forks Claude Code, and the agent's pane runs {}",
                agent
                    .agent
                    .as_deref()
                    .unwrap_or("an agent Herdr does not name")
            ));
        }
        let hooks = self
            .hooks
            .as_deref()
            .ok_or("the reviewer hears no agent's hooks: its log says why")?;
        let session = agent
            .agent_session
            .as_ref()
            .ok_or("Herdr reports no session for the agent")?;
        Ok((hooks, &session.value))
    }
}

impl Drop for ClaudeForks {
    /// Lets the discards under way finish, so no transcript of a discarded fork is left.
    fn drop(&mut self) {
        let discards =
            std::mem::take(&mut *self.discards.lock().unwrap_or_else(PoisonError::into_inner));
        for discard in discards {
            let _ = discard.join();
        }
    }
}

/// Whether Herdr reports `agent` as Claude Code.
fn runs_claude(agent: &Agent) -> bool {
    agent.agent.as_deref() == Some("claude")
}

/// Whether a process of the pane is Claude Code.
fn is_claude(process: &PaneProcess) -> bool {
    process.name == "claude"
        || process
            .argv
            .as_ref()
            .and_then(|argv| argv.first())
            .is_some_and(|program| program.rsplit('/').next() == Some("claude"))
}

impl ForkHost for ClaudeForks {
    fn watch(&self, pane: &PaneId, report: StatusReport) -> PaneWatch {
        PaneWatch::herdr(&self.herdr, pane, self.waits.resubscribe, report)
    }

    fn session(&self, agent: &Agent) -> Option<String> {
        agent
            .agent_session
            .as_ref()
            .map(|session| session.value.clone())
    }

    fn unhooked(&self, agent: &Agent) -> Option<String> {
        // An agent run-ahead cannot fork for another reason has that reason, which `point`
        // gives.
        self.forkable(agent)
            .is_ok_and(|(hooks, _)| hooks.hooked(&self.herdr, agent).is_none())
            .then(|| "the agent runs without the reviewer's Claude Code plugin: restart it".into())
    }

    fn can_move(&self, agent: &Agent) -> bool {
        self.forkable(agent)
            .is_ok_and(|(hooks, _)| hooks.hooked(&self.herdr, agent).is_some())
    }

    fn unforkable(&self, agent: &Agent) -> Option<String> {
        self.forkable(agent).err()
    }

    fn point(&self, agent: &Agent) -> Result<ForkPoint, String> {
        let session = self.forkable(agent)?.1.to_owned();
        let processes = self
            .herdr
            .pane_process_info(&agent.pane_id)
            .map_err(|error| error.to_string())?;
        let pid = processes
            .foreground_processes
            .iter()
            .find(|process| is_claude(process))
            .map(|process| process.pid)
            .ok_or("no Claude Code process runs in the agent's pane")?;
        let pane = PaneClaude::read(pid)?;
        let transcripts = pane.transcripts();
        let last = transcripts
            .last_entry(&session)
            .ok_or_else(|| format!("the session {session} has no transcript"))?;
        Ok(ForkPoint {
            session,
            entry: last.entry,
            model: last.model.or(pane.model),
            host: fields::point(transcripts.root(), &pane.command),
        })
    }

    fn last_entry(&self, point: &ForkPoint) -> Option<String> {
        Transcripts::at(fields::transcripts(&point.host)?)
            .last_entry(&point.session)?
            .entry
    }

    fn create(&self, point: &ForkPoint) -> Result<OwnedFork, String> {
        let transcripts =
            fields::transcripts(&point.host).ok_or("the point names no transcript directory")?;
        Ok(OwnedFork {
            session: uuid::Uuid::new_v4().to_string(),
            host: fields::fork(transcripts),
        })
    }

    fn start(
        &self,
        start: ForkStart<'_>,
        prompt: String,
        ended: Box<dyn FnOnce(ForkEnd) + Send>,
    ) -> Result<HostFields, String> {
        let point = start.point;
        let mut command =
            fields::command(&point.host).ok_or("the point names no Claude Code command")?;
        command.arguments.extend(
            [
                "-p",
                "--resume",
                &point.session,
                "--fork-session",
                "--session-id",
                start.fork.session,
                "--output-format",
                "stream-json",
                "--verbose",
                "--settings",
                &self.tools.settings(),
                "--allowedTools",
            ]
            .into_iter()
            .chain(SUBMITS.iter().copied())
            .map(OsString::from),
        );
        if let Some(model) = &point.model {
            command.arguments.extend(["--model".into(), model.into()]);
        }
        let program = command.program.display().to_string();
        let (live, session) = (Arc::clone(&self.live), start.fork.session.to_owned());
        let ended = Box::new(move |end| {
            live.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&session);
            ended(end);
        });
        let tally = StreamTally::new(ended);
        let answered = tally.submit_answered();
        let fork = self
            .launcher
            .spawn(command, prompt, Box::new(tally))
            .map_err(|error| format!("cannot start {program}: {error}"))?;
        let stamp = fork.stamp();
        self.live().insert(
            start.fork.session.to_owned(),
            LiveFork {
                process: Arc::new(fork),
                answered,
            },
        );
        Ok(fields::started(stamp))
    }

    fn discard(&self, fork: ForkTrace<'_>, done: Box<dyn FnOnce() + Send>) {
        let running = self.live().remove(fork.session);
        let (session, process) = (fork.session.to_owned(), fields::process(fork.host));
        let transcripts = fields::transcripts(fork.host).map(Transcripts::at);
        let discard = thread::spawn(move || {
            ForkProcess::of(running, process).stop(&session);
            if let Some(transcripts) = transcripts {
                transcripts.delete(&session);
            }
            done();
        });
        let mut discards = self.discards.lock().unwrap_or_else(PoisonError::into_inner);
        discards.retain(|discard| !discard.is_finished());
        discards.push(discard);
    }

    fn switch(
        &self,
        switch: SwitchTo<'_>,
        done: Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>,
    ) {
        let fork = ForkProcess::of(
            self.live().remove(switch.fork.session),
            fields::process(switch.fork.host),
        );
        let (herdr, pane, session) = (
            self.herdr.clone(),
            switch.pane.clone(),
            switch.fork.session.to_owned(),
        );
        let (waits, hooks) = (self.waits, self.hooks.clone());
        thread::spawn(move || {
            let switch = Switch {
                pane: AgentPane {
                    herdr: &herdr,
                    pane: &pane,
                },
                session: &session,
                fork,
                waits,
                hooks: hooks.as_deref(),
            };
            done(switch.run());
        });
    }

    fn resume(
        &self,
        pane: &PaneId,
        _from: ForkTrace<'_>,
        session: &str,
        done: Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>,
    ) {
        let (herdr, pane, session) = (self.herdr.clone(), pane.clone(), session.to_owned());
        let (waits, hooks) = (self.waits, self.hooks.clone());
        thread::spawn(move || {
            let resume = Resume {
                pane: AgentPane {
                    herdr: &herdr,
                    pane: &pane,
                },
                session: &session,
                limit: waits.settle,
                poll: waits.resume_poll,
                hooks: hooks.as_deref(),
            };
            done(resume.run());
        });
    }

    fn log(&self, line: &str) {
        self.log.write(line);
    }
}
