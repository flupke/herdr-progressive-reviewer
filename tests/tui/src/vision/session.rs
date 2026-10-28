//! One vision session: the reviewer running in a workspace of the user's Herdr, on a scratch
//! repository, beside a stand-in agent that run-ahead can fork. Each action waits for the frame
//! that shows the reviewer's reaction, which Herdr reports as an event; the stand-in agent and its
//! forks report on the session's event socket.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail, ensure};
use review_repository::repository::RepoType;
use serde_json::{Value, json};
use vision_signal::{FrameMarker, acknowledgement_request};

use herdr_client::client::{EventCanceller, HerdrClient};
use herdr_client::protocol::{AgentPort, PaneId};
use review_test_support::stand_in::{StandInCommand, StandInEvent, StandInEvents, StandInRole};

use super::agent::ScriptedAgent;
use super::forks;
use super::frames::{FrameWatch, Stalled};
use super::keys::KeyInput;
use super::scratch::{Scratch, Seed};
use super::screen::{Cell, Screen};
use super::screenshot;
use super::workspace::{Closing, Panes, Workspace, string};

/// How long the reviewer may take to read an input and paint its reaction, or to paint its first
/// frame. It fires only when something went wrong.
const REACTION_GUARD: Duration = Duration::from_secs(30);

/// The share of the tab's width the reviewer takes beside its stand-in agent.
const REVIEWER_SHARE: f64 = 0.75;

/// The title the stand-in agent shows outside its turns: Claude Code's, which Herdr reads as
/// idle.
const IDLE_TITLE: &str = "✳ Ready";

/// Claude Code's title while it works, which Herdr reads as working.
const WORKING_TITLE: &str = "⠋ Working";

/// What a session starts on.
pub(crate) struct StartOptions {
    pub(crate) repository: RepoType,
    pub(crate) seed: Seed,
    /// Whether Herdr shows the session's workspace at once.
    pub(crate) focus: bool,
    /// Whether the stand-in agent runs the hooks of the reviewer's Claude Code plugin, which
    /// run-ahead needs.
    pub(crate) agent_hooks: bool,
}

/// One input to the reviewer.
pub(crate) enum Input<'a> {
    /// Keys pressed in order, by Herdr's names.
    Keys(&'a [String]),
    /// Text pasted as the user's terminal pastes it.
    Text(&'a str),
    /// A left click on a cell.
    Click(Cell),
    /// A drag with the left button between two cells, Alt held when `alt`.
    Drag { from: Cell, to: Cell, alt: bool },
}

pub(crate) struct Session {
    // Stop listening, then close the workspace, then stop listening to the stand-ins, then remove
    // the scratch repository.
    watch: FrameWatch,
    workspace: Workspace,
    panes: Panes,
    /// What the stand-in agent and its forks report.
    events: StandInEvents,
    scratch: Scratch,
    reviewer_program: PathBuf,
    /// Whether the stand-in agent runs the hooks of the reviewer's Claude Code plugin.
    agent_hooks: bool,
    agent: ScriptedAgent,
    /// The latest acknowledgement request sent.
    requests: u64,
}

impl Session {
    /// Open a workspace in `herdr` on a new scratch repository, with the reviewer `reviewer` and
    /// its stand-in agent in its tab, and wait for the reviewer's first frame.
    pub(crate) fn start(
        herdr: &HerdrClient,
        reviewer: PathBuf,
        options: &StartOptions,
        directory: PathBuf,
    ) -> Result<(Self, Screen)> {
        let scratch = Scratch::create(options.repository, &options.seed, directory)?;
        scratch.install_agent(&reviewer)?;
        let events = StandInEvents::listen(&scratch.events_socket());
        let (mut workspace, tab) = Workspace::create(herdr, scratch.root(), options.focus)?;
        let watch = FrameWatch::start(herdr, workspace.id().to_owned())?;
        let panes = Self::open_panes(
            &mut workspace,
            &tab,
            &scratch,
            &reviewer,
            options.agent_hooks,
            &events,
        )?;
        let agent = ScriptedAgent::new(scratch.turns(), scratch.mcp_port());
        let session = Self {
            watch,
            workspace,
            panes,
            events,
            scratch,
            reviewer_program: reviewer,
            agent_hooks: options.agent_hooks,
            agent,
            requests: 0,
        };
        let screen = session.first_screen()?;
        Ok((session, screen))
    }

    /// Replace the tab's layout with a new reviewer and stand-in agent, on a new session, and
    /// return once the agent shows its idle title. The agent runs the plugin's hooks with
    /// `agent_hooks`.
    fn open_panes(
        workspace: &mut Workspace,
        tab: &str,
        scratch: &Scratch,
        reviewer: &Path,
        agent_hooks: bool,
        events: &StandInEvents,
    ) -> Result<Panes> {
        let herdr = workspace.herdr();
        let session = format!(
            "vision-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        let from = events.try_received().map_err(|error| anyhow!(error))?.len();
        let applied = herdr.request(
            "layout.apply",
            &json!({
                "tab_id": tab,
                "tab_label": "reviewer",
                "root": {
                    "type": "split",
                    "direction": "right",
                    "ratio": REVIEWER_SHARE,
                    "first": {
                        "type": "pane",
                        "label": "reviewer",
                        "cwd": scratch.root(),
                        "command": Scratch::reviewer_command(reviewer),
                        "env": scratch.reviewer_environment(workspace.id(), herdr.socket_path())?,
                    },
                    "second": {
                        "type": "pane",
                        "label": "stand-in agent",
                        "cwd": scratch.root(),
                        "command": scratch.agent_command(),
                        "env": scratch.agent_environment(reviewer, &session, agent_hooks),
                    },
                },
            }),
        )?;
        let layout = &applied["layout"];
        let panes = Panes {
            tab: string(&layout["tab_id"])?,
            reviewer: string(&layout["root"]["first"]["pane_id"])?,
            agent: string(&layout["root"]["second"]["pane_id"])?,
        };
        workspace.opened(&panes);
        // The agent that connects from now on is the new one: an agent the layout replaced
        // keeps its connection until its process ends.
        events
            .try_wait_until("the stand-in agent", REACTION_GUARD, |received| {
                received[from..].iter().any(|reported| {
                    reported.from == StandInRole::Agent
                        && matches!(reported.event, StandInEvent::Hello { .. })
                })
            })
            .map_err(|error| anyhow!(error))?;
        events
            .try_send(
                &StandInRole::Agent,
                &StandInCommand::ShowTitle {
                    title: IDLE_TITLE.into(),
                },
                REACTION_GUARD,
            )
            .map_err(|error| anyhow!(error))?;
        Ok(panes)
    }

    /// The reviewer's first screen, once Herdr also knows the stand-in agent: Explore prompts it
    /// from the start.
    fn first_screen(&self) -> Result<Screen> {
        self.watch
            .frames()
            .wait_agent(&self.panes.agent, REACTION_GUARD)
            .map_err(|error| anyhow!("Herdr did not detect the stand-in agent: {error}"))?;
        let marker = self
            .wait_frame(|_| true)
            .map_err(|error| anyhow!("the reviewer did not start: {error}"))?;
        self.read(&marker, false)
    }

    /// What `start` and `reopen` report besides the screen.
    pub(crate) fn describe(&self) -> Value {
        json!({
            "workspace": self.workspace.id(),
            "tab": self.panes.tab,
            "reviewer_pane": self.panes.reviewer,
            "agent_pane": self.panes.agent,
            "repository": self.scratch.root(),
            "directory": self.scratch.directory(),
            "agent_swallows_prompts": self.scratch.stand_in().swallow(),
            "agent_hooks": self.agent_hooks,
            "run_ahead_log": self.scratch.run_ahead_log(),
            "claude_config": self.scratch.claude_config(),
            "runtime": self.scratch.runtime(),
        })
    }

    pub(crate) fn directory(&self) -> &Path {
        self.scratch.directory()
    }

    /// The screen of the latest frame, with its styles when `styled`.
    pub(crate) fn screen(&self, styled: bool) -> Result<Screen> {
        let marker = self
            .watch
            .frames()
            .pane(&self.panes.reviewer)
            .latest
            .context("the reviewer has painted no frame")?;
        self.read(&marker, styled)
    }

    /// Send `input`, and return the screen once the reviewer has read it and painted its
    /// reaction.
    pub(crate) fn act(&mut self, input: &Input<'_>) -> Result<Screen> {
        let pane = self.panes.reviewer.clone();
        match input {
            Input::Keys(keys) => {
                ensure!(!keys.is_empty(), "no key to press");
                // Herdr checks every key of one call before it writes any, so a run of keys it
                // names goes in one call.
                let mut named = Vec::new();
                for key in *keys {
                    match KeyInput::parse(key) {
                        KeyInput::Herdr(name) => named.push(name),
                        KeyInput::Bytes(bytes) => {
                            self.send_keys(&pane, &std::mem::take(&mut named))?;
                            self.send_text(&pane, &bytes)?;
                        }
                    }
                }
                self.send_keys(&pane, &named)?;
            }
            Input::Text(text) => self.send_text(&pane, &format!("\x1b[200~{text}\x1b[201~"))?,
            Input::Click(cell) => {
                self.screen(false)?.contains(*cell)?;
                self.send_text(&pane, &cell.left_click())?;
            }
            Input::Drag { from, to, alt } => {
                let screen = self.screen(false)?;
                screen.contains(*from)?;
                screen.contains(*to)?;
                self.send_text(&pane, &from.left_drag(*to, *alt))?;
            }
        }
        self.settle()
    }

    /// Ask the reviewer for an acknowledgement after everything sent so far, and return the
    /// screen of the frame that names it.
    pub(crate) fn settle(&mut self) -> Result<Screen> {
        self.requests += 1;
        let request = self.requests;
        let pane = self.panes.reviewer.clone();
        self.send_text(
            &pane,
            &format!("\x1b[200~{}\x1b[201~", acknowledgement_request(request)),
        )?;
        let marker = self
            .wait_frame(|marker| marker.acknowledged >= request)
            .map_err(|error| anyhow!("the reviewer did not react to the input: {error}"))?;
        self.read(&marker, false)
    }

    /// Wait until the screen shows `text`, or no longer shows it when `absent`, for at most
    /// `guard`, reading the screen after each frame.
    pub(crate) fn wait_for(&self, text: &str, absent: bool, guard: Duration) -> Result<Screen> {
        ensure!(!text.is_empty(), "the text to wait for is empty");
        let deadline = std::time::Instant::now() + guard;
        let mut screen = self.screen(false)?;
        loop {
            if screen.shows(text) != absent {
                return Ok(screen);
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let frame = screen.frame;
            let marker = match self
                .watch
                .frames()
                .wait(&self.panes.reviewer, remaining, |marker| {
                    marker.frame > frame
                }) {
                Ok(marker) => marker,
                Err(Stalled::Guard(_)) => bail!(
                    "the screen {} {text:?} after {} ms; it shows:\n{}",
                    if absent {
                        "still shows"
                    } else {
                        "does not show"
                    },
                    guard.as_millis(),
                    screen.compact()
                ),
                Err(stalled) => bail!("{stalled}; the screen shows:\n{}", screen.compact()),
            };
            screen = self.read(&marker, false)?;
        }
    }

    /// Classify as insignificant the hunks of `path` that hold `lines`, as Jev would, press
    /// `rf`, and return the screen once Jev's classification has finished, or `rf` refused.
    pub(crate) fn jev(&mut self, path: &str, lines: &[u32]) -> Result<Screen> {
        fs::write(
            self.scratch.jev_script(),
            serde_json::to_vec(&json!({"insignificant": [{"path": path, "lines": lines}]}))?,
        )?;
        let pressed = self.act(&Input::Keys(&["r".into(), "f".into()]))?;
        if pressed.shows("Jev: classifying") {
            return self.wait_for("Jev: classifying", true, REACTION_GUARD);
        }
        Ok(pressed)
    }

    pub(crate) fn next_turn(&mut self, after: Option<u64>, guard: Duration) -> Result<Value> {
        self.agent.next_turn(after, guard)
    }

    /// Answer turn `turn` with `tool`, and return the tool's result with the screen once the
    /// reviewer has shown it.
    pub(crate) fn reply(
        &mut self,
        turn: u64,
        tool: &str,
        arguments: Value,
    ) -> Result<(Value, Screen)> {
        let result = self.agent.reply(turn, tool, arguments)?;
        Ok((result, self.settle()?))
    }

    /// The forks run-ahead started, once `count` did when it is given, waiting up to `guard`;
    /// the agent's sessions; and run-ahead's log.
    pub(crate) fn forks(&self, count: Option<usize>, guard: Duration) -> Result<Value> {
        if let Some(count) = count {
            self.events
                .try_wait_until(&format!("{count} forks"), guard, |received| {
                    forks::forks(received).len() >= count
                })
                .map_err(|error| anyhow!(error))?;
        }
        // A wait whose condition held at once took in none of the events that came since.
        let received = self.events.try_received().map_err(|error| anyhow!(error))?;
        Ok(json!({
            "forks": forks::forks(&received),
            "agent": forks::agent_sessions(&received),
            "log": fs::read_to_string(self.scratch.run_ahead_log()).unwrap_or_default(),
        }))
    }

    /// Submit the turn of the fork `session` with `tool`, as that fork: the reviewer's tool on
    /// its real endpoint, with the identity of the fork's prompt where `arguments` leave it out;
    /// then the fork's output says that its submit has its answer. Returns the tool's result once
    /// the fork said so, with the screen once the reviewer has shown it.
    pub(crate) fn fork_submit(
        &mut self,
        session: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<(Value, Screen)> {
        let received = self.events.try_received().map_err(|error| anyhow!(error))?;
        let prompt = forks::forks(&received)
            .into_iter()
            .find(|fork| fork.session == session)
            .with_context(|| format!("no fork has the session {session}"))?
            .prompt;
        let result = self.agent.reply_as_fork(&prompt, tool, arguments)?;
        if result["isError"] == true {
            return Ok((result, self.settle()?));
        }
        let fork = StandInRole::Fork {
            session: session.to_owned(),
        };
        self.events
            .try_send(&fork, &StandInCommand::Submit, REACTION_GUARD)
            .map_err(|error| anyhow!(error))?;
        self.events
            .try_wait_until("the fork's submit", REACTION_GUARD, |received| {
                received.iter().any(|reported| {
                    reported.from == fork && reported.event == StandInEvent::ForkSubmitted
                })
            })
            .map_err(|error| anyhow!(error))?;
        Ok((result, self.settle()?))
    }

    /// End the turn of the fork `session` without a submit, or make it crash when `crash`: it
    /// exits with a failure and no result. Returns the screen once the fork's process ended.
    pub(crate) fn fork_end(&mut self, session: &str, crash: bool) -> Result<Screen> {
        let received = self.events.try_received().map_err(|error| anyhow!(error))?;
        let fork = forks::forks(&received)
            .into_iter()
            .find(|fork| fork.session == session)
            .with_context(|| format!("no fork has the session {session}"))?;
        ensure!(!fork.exited, "the fork {session} has exited already");
        let fork = StandInRole::Fork {
            session: session.to_owned(),
        };
        let command = if crash {
            StandInCommand::Crash
        } else {
            StandInCommand::End
        };
        self.events
            .try_send(&fork, &command, REACTION_GUARD)
            .map_err(|error| anyhow!(error))?;
        self.events
            .try_wait_until("the fork's exit", REACTION_GUARD, |received| {
                received.iter().any(|reported| {
                    reported.from == fork && reported.event == StandInEvent::Disconnected
                })
            })
            .map_err(|error| anyhow!(error))?;
        self.settle()
    }

    /// Make the agent's next `/resume` one that Herdr and the reviewer's hooks never hear of, as
    /// when Claude Code's session hooks fail, or hear of only `heard_after` later: the next switch
    /// to a fork waits for them, in vain or that long.
    pub(crate) fn switch_unheard(&self, heard_after: Option<Duration>) -> Result<()> {
        let stand_in = self.scratch.stand_in();
        fs::create_dir_all(stand_in.path())?;
        let late = heard_after.map_or_else(String::new, |late| late.as_millis().to_string());
        fs::write(stand_in.unreported_resume(), late)?;
        Ok(())
    }

    /// Show the stand-in agent working, with Claude Code's working title, or idle again, and
    /// return once Herdr reports it so.
    pub(crate) fn agent_working(&mut self, working: bool) -> Result<Screen> {
        let pane = PaneId(self.panes.agent.clone());
        let herdr = self.workspace.herdr();
        let canceller = EventCanceller::default();
        let statuses = herdr.subscribe_agent_status(&pane, &canceller)?;
        let title = if working { WORKING_TITLE } else { IDLE_TITLE };
        self.events
            .try_send(
                &StandInRole::Agent,
                &StandInCommand::ShowTitle {
                    title: title.into(),
                },
                REACTION_GUARD,
            )
            .map_err(|error| anyhow!(error))?;
        let (reported, reports) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            statuses.forward(|status| {
                let shown = status.started_on_prompt() == working;
                if shown {
                    let _ = reported.send(());
                }
                !shown
            })
        });
        let shown = reports.recv_timeout(REACTION_GUARD);
        canceller.cancel();
        shown.map_err(|_| {
            anyhow!(
                "Herdr did not report the agent {} in {} ms",
                if working { "working" } else { "idle" },
                REACTION_GUARD.as_millis()
            )
        })?;
        self.settle()
    }

    /// Type `text` in the stand-in agent's pane and submit it, as its user would, and return
    /// once the agent read it: a prompt moves its conversation on.
    pub(crate) fn agent_prompt(&mut self, text: &str) -> Result<Screen> {
        ensure!(
            !text.trim().is_empty(),
            "the agent's prompt is empty: the agent would not read it"
        );
        let from = self
            .events
            .try_received()
            .map_err(|error| anyhow!(error))?
            .len();
        let pane = self.panes.agent.clone();
        self.send_text(&pane, &format!("\x1b[200~{text}\x1b[201~"))?;
        self.send_keys(&pane, &["enter".to_owned()])?;
        self.events
            .try_wait_until("the agent's prompt", REACTION_GUARD, |received| {
                received[from..].iter().any(|reported| {
                    reported.from == StandInRole::Agent
                        && matches!(
                            reported.event,
                            StandInEvent::PromptReceived { .. }
                                | StandInEvent::PromptBlocked { .. }
                                | StandInEvent::ResumeReceived { .. }
                        )
                })
            })
            .map_err(|error| anyhow!(error))?;
        self.settle()
    }

    pub(crate) fn explore_page(&self) -> Result<String> {
        let environment = self
            .scratch
            .reviewer_environment(self.workspace.id(), self.workspace.herdr().socket_path())?;
        self.scratch
            .open_explore_page(&self.reviewer_program, &environment)
    }

    /// Save a PNG of the latest screen in the session directory.
    pub(crate) fn screenshot(&self) -> Result<(Screen, screenshot::Screenshot)> {
        let screen = self.screen(true)?;
        let directory = self.scratch.directory().join("screenshots");
        fs::create_dir_all(&directory)?;
        let saved = screenshot::save(
            &screen,
            &directory.join(format!("frame-{}.png", screen.frame)),
        )?;
        Ok((screen, saved))
    }

    /// Start the reviewer and its stand-in agent again, in new panes, on the same repository and
    /// state; the agent runs the plugin's hooks as `agent_hooks` says, or as before. With
    /// `workspace`, both run in that workspace of the repository from now on (see
    /// [`Scratch::enter_workspace`]).
    pub(crate) fn reopen(
        &mut self,
        agent_hooks: Option<bool>,
        workspace: Option<&str>,
    ) -> Result<Screen> {
        self.stop_reviewer()?;
        if let Some(workspace) = workspace {
            self.scratch.enter_workspace(workspace)?;
        }
        self.agent_hooks = agent_hooks.unwrap_or(self.agent_hooks);
        self.panes = Self::open_panes(
            &mut self.workspace,
            &self.panes.tab,
            &self.scratch,
            &self.reviewer_program,
            self.agent_hooks,
            &self.events,
        )?;
        self.first_screen()
    }

    /// Stop the reviewer, and wait until Herdr reports its process ended: the next one binds
    /// the same MCP port.
    fn stop_reviewer(&self) -> Result<()> {
        let frames = self.watch.frames();
        if frames.pane(&self.panes.reviewer).exited {
            return Ok(());
        }
        let processes = self
            .workspace
            .herdr()
            .pane_process_info(&PaneId(self.panes.reviewer.clone()))?
            .foreground_processes;
        ensure!(!processes.is_empty(), "the reviewer's pane runs no process");
        for process in &processes {
            terminate(process.pid)?;
        }
        frames
            .wait_exit(&self.panes.reviewer, REACTION_GUARD)
            .map_err(|error| anyhow!("the reviewer did not stop: {error}"))
    }

    /// Close the session's workspace, if it is still the session's, and remove the scratch
    /// repository.
    pub(crate) fn stop(mut self) -> Closing {
        self.workspace.close()
    }

    fn wait_frame(&self, until: impl FnMut(&FrameMarker) -> bool) -> Result<FrameMarker, Stalled> {
        self.watch
            .frames()
            .wait(&self.panes.reviewer, REACTION_GUARD, until)
    }

    fn read(&self, marker: &FrameMarker, styled: bool) -> Result<Screen> {
        let text = self.read_pane(false)?;
        let styled = styled.then(|| self.read_pane(true)).transpose()?;
        Ok(Screen::new(marker, text, styled))
    }

    fn read_pane(&self, styled: bool) -> Result<String> {
        let read = self.workspace.herdr().request(
            "pane.read",
            &json!({
                "pane_id": self.panes.reviewer,
                "source": "visible",
                "format": if styled { "ansi" } else { "text" },
                "strip_ansi": !styled,
            }),
        )?;
        string(&read["read"]["text"])
    }

    fn send_keys(&self, pane: &str, keys: &[String]) -> Result<()> {
        if keys.is_empty() {
            return Ok(());
        }
        self.workspace
            .herdr()
            .request("pane.send_keys", &json!({"pane_id": pane, "keys": keys}))?;
        Ok(())
    }

    fn send_text(&self, pane: &str, text: &str) -> Result<()> {
        self.workspace
            .herdr()
            .request("pane.send_text", &json!({"pane_id": pane, "text": text}))?;
        Ok(())
    }
}

/// Ask process `pid` to stop. A short-lived child of the reviewer, such as a jj command, can end
/// between the listing of the pane's processes and its signal: a process that is gone counts as
/// stopped. `ps` tells whether it is gone, which `kill -0` cannot when it is not allowed to signal.
fn terminate(pid: u32) -> Result<()> {
    let pid = pid.to_string();
    let succeeds = |program: &str, arguments: &[&str]| {
        std::process::Command::new(program)
            .args(arguments)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
    };
    ensure!(
        succeeds("kill", &["-TERM", &pid])? || !succeeds("ps", &["-p", &pid])?,
        "cannot stop the reviewer's process {pid}"
    );
    Ok(())
}

#[cfg(test)]
#[path = "session.tests.rs"]
mod tests;
