//! [`ClaudeForks`]: run-ahead's forks of a Claude Code agent in a Herdr pane.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use agent_fork::{Launcher, ProcessStamp, Wrapper};
use herdr_client::client::{EventCanceller, HerdrClient};
use herdr_client::protocol::{Agent, AgentPort, AgentStatus, PaneId, PaneProcess};
use review_run_ahead::{
    ForkEnd, ForkHost, ForkPoint, ForkStart, ForkTrace, PaneWatch, StatusReport, SwitchFailure,
    SwitchTo,
};

use crate::guard::SUBMITS;
use crate::pane::PaneClaude;
use crate::stream::StreamTally;
use crate::switch::{AgentPane, ForkProcess, LiveFork, Resume, Switch};
use crate::transcripts::Transcripts;

/// How long the host waits for Herdr and for its forks. Production uses the defaults; tests set
/// them small.
#[derive(Clone, Copy, Debug)]
pub struct ForkWaits {
    /// How long a switch waits for the answer to the fork's submit to reach the fork, which
    /// then writes it in its transcript, before it stops the fork anyway.
    pub answer: Duration,
    /// How long a switch waits for Herdr to report the agent on the fork's session, ready for
    /// a prompt, once the agent was told to resume it.
    pub resume: Duration,
    /// How often a switch asks Herdr where the agent stands while it waits: Herdr sends no
    /// event when an agent's session changes.
    pub resume_poll: Duration,
    /// How long a status watch waits before it subscribes again after Herdr dropped it.
    pub resubscribe: Duration,
}

impl Default for ForkWaits {
    fn default() -> Self {
        Self {
            answer: Duration::from_secs(10),
            resume: Duration::from_secs(20),
            resume_poll: Duration::from_millis(100),
            resubscribe: Duration::from_secs(1),
        }
    }
}

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
    /// Run-ahead's log; `None` writes none.
    log: Option<PathBuf>,
    waits: ForkWaits,
    /// The forks that run, by session.
    live: Arc<Mutex<HashMap<String, LiveFork>>>,
    /// The discards under way, which stop a fork and delete its transcript.
    discards: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl ClaudeForks {
    /// Forks of the agents `herdr` reports, started through `tools`, logged to `log`, waiting
    /// on Herdr and on the forks as `waits` say.
    pub fn new(
        herdr: HerdrClient,
        tools: ForkTools,
        log: Option<PathBuf>,
        waits: ForkWaits,
    ) -> Self {
        Self {
            launcher: Launcher::start(tools.wrapper()),
            herdr,
            tools,
            log,
            waits,
            live: Arc::default(),
            discards: Mutex::default(),
        }
    }

    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<String, LiveFork>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
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

/// Whether a process of the pane is Claude Code.
fn is_claude(process: &PaneProcess) -> bool {
    process.name == "claude"
        || process
            .argv
            .as_ref()
            .and_then(|argv| argv.first())
            .is_some_and(|program| program.rsplit('/').next() == Some("claude"))
}

/// Ends a status watch when dropped.
struct CancelOnDrop(EventCanceller);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl ForkHost for ClaudeForks {
    fn watch(&self, pane: &PaneId, report: StatusReport) -> PaneWatch {
        let canceller = EventCanceller::default();
        let (herdr, pane, stop) = (self.herdr.clone(), pane.clone(), canceller.clone());
        let resubscribe = self.waits.resubscribe;
        thread::spawn(move || {
            let mut dropped = false;
            while !stop.is_cancelled() {
                // The agent may have worked while the subscription was down: say so, and the
                // status that follows tells where it stands now.
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
        PaneWatch::new(CancelOnDrop(canceller))
    }

    fn point(&self, agent: &Agent) -> Result<ForkPoint, String> {
        if !cfg!(target_os = "linux") {
            // Only Linux has the parent-death signal that keeps a fork from outliving the
            // reviewer when the reviewer runs outside a terminal.
            return Err("run-ahead forks only on Linux".into());
        }
        if agent.agent.as_deref() != Some("claude") {
            return Err(format!(
                "run-ahead forks Claude Code, and the agent's pane runs {}",
                agent
                    .agent
                    .as_deref()
                    .unwrap_or("an agent Herdr does not name")
            ));
        }
        let session = agent
            .agent_session
            .as_ref()
            .map(|session| session.value.clone())
            .ok_or("Herdr reports no session for the agent")?;
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
            transcripts: transcripts.root().to_owned(),
            model: last.model.or(pane.model),
            command: pane.command,
        })
    }

    fn last_entry(&self, point: &ForkPoint) -> Option<String> {
        Transcripts::at(point.transcripts.clone())
            .last_entry(&point.session)?
            .entry
    }

    fn start(
        &self,
        start: ForkStart<'_>,
        ended: Box<dyn FnOnce(ForkEnd) + Send>,
    ) -> Result<ProcessStamp, String> {
        let point = start.point;
        let mut command = point.command.clone();
        command.arguments.extend(
            [
                "-p",
                "--resume",
                &point.session,
                "--fork-session",
                "--session-id",
                start.session,
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
        let (live, session) = (Arc::clone(&self.live), start.session.to_owned());
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
            .spawn(command, start.prompt, Box::new(tally))
            .map_err(|error| format!("cannot start {program}: {error}"))?;
        let stamp = fork.stamp();
        self.live().insert(
            start.session.to_owned(),
            LiveFork {
                process: Arc::new(fork),
                answered,
            },
        );
        Ok(stamp)
    }

    fn discard(&self, fork: ForkTrace<'_>, done: Box<dyn FnOnce() + Send>) {
        let running = self.live().remove(fork.session);
        let (session, process) = (fork.session.to_owned(), fork.process);
        let transcripts = Transcripts::at(fork.transcripts.to_owned());
        let discard = thread::spawn(move || {
            ForkProcess::of(running, process).stop(&session);
            transcripts.delete(&session);
            done();
        });
        let mut discards = self.discards.lock().unwrap_or_else(PoisonError::into_inner);
        discards.retain(|discard| !discard.is_finished());
        discards.push(discard);
    }

    fn input_is_empty(&self, pane: &PaneId) -> Result<bool, String> {
        let pane = AgentPane {
            herdr: &self.herdr,
            pane,
        };
        pane.input_box_empty()?
            .ok_or_else(|| "the agent's screen shows no input box".to_owned())
    }

    fn switch(
        &self,
        switch: SwitchTo<'_>,
        done: Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>,
    ) {
        let fork = ForkProcess::of(self.live().remove(switch.fork.session), switch.fork.process);
        let (herdr, pane, session) = (
            self.herdr.clone(),
            switch.pane.clone(),
            switch.fork.session.to_owned(),
        );
        let waits = self.waits;
        thread::spawn(move || {
            let switch = Switch {
                pane: AgentPane {
                    herdr: &herdr,
                    pane: &pane,
                },
                session: &session,
                fork,
                waits,
            };
            done(switch.run());
        });
    }

    fn resume(
        &self,
        pane: &PaneId,
        session: &str,
        done: Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>,
    ) {
        let (herdr, pane, session) = (self.herdr.clone(), pane.clone(), session.to_owned());
        let waits = self.waits;
        thread::spawn(move || {
            let resume = Resume {
                pane: AgentPane {
                    herdr: &herdr,
                    pane: &pane,
                },
                session: &session,
                waits,
            };
            done(resume.run());
        });
    }

    fn log(&self, line: &str) {
        let Some(path) = &self.log else {
            return;
        };
        let now = time::OffsetDateTime::now_utc();
        let line = format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} UTC [pid {}] {line}\n",
            now.year(),
            u8::from(now.month()),
            now.day(),
            now.hour(),
            now.minute(),
            now.second(),
            now.millisecond(),
            std::process::id(),
        );
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            // One write per line, so the lines of several threads do not interleave.
            let _ = file.write_all(line.as_bytes());
        }
    }
}
