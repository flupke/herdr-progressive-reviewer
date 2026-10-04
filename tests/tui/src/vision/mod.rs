mod agent;
mod capture;
mod command;
mod frame;
mod input;
mod stream;
mod viewer;

pub use herdr_client::protocol::SplitDirection;
pub use viewer::{Placement, view};

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use review_repository::repository::RepoType;
use serde::Serialize;
use serde_json::{Value, json};
use tui_test::{
    KeyAction, MouseAction, MouseOptions, Operation, OperationResult, ScreenshotResult, Session,
};

use crate::fixture::{ReviewWorkspace, SESSION_SIZE, VisionFiles};
use capture::Capture;
use command::Command;
use frame::{Frame, Frames};
use input::{Input, Next};

/// Options for a persistent, isolated terminal exploration session.
pub struct Options {
    pub repository_type: RepoType,
    pub directory: PathBuf,
    pub json: bool,
    /// A named pipe to read commands from instead of standard input. Each
    /// writer may close it; the driver reopens it for the next one.
    pub commands: Option<PathBuf>,
    /// Where to open the live viewer pane when the driver runs inside Herdr;
    /// `None` keeps the session headless.
    pub viewer: Option<Placement>,
    /// Stop when no command comes for this long; `None` waits forever.
    pub stop_after_idle: Option<Duration>,
}

/// The longest a command may wait for the screen.
const MAX_WAIT_MS: u64 = 30_000;

/// What a reported screen says about the command that waited for it.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ScreenStatus {
    /// A newer frame arrived.
    Changed,
    /// No newer frame arrived in time.
    Unchanged,
    /// The awaited text showed.
    Shown,
    /// The awaited text did not show in time.
    Timeout,
    /// The reviewer has exited.
    Exited,
}

struct LiveReviewer {
    session: Session,
    _capture: Capture,
}

impl LiveReviewer {
    fn start(workspace: &ReviewWorkspace, frames: &Arc<Frames>, run: u64) -> Result<Self> {
        let session = workspace.open_vision(&VisionFiles {
            recording: frames.directory().join(format!("recording-{run}")),
            jev_script: jev_script(frames),
            turns: turns(frames),
            browser: frames.directory().to_owned(),
        })?;
        let capture = (|| {
            let recording = session
                .recording_path()
                .context("tui-test did not start a recording")?;
            Capture::start(&recording, Arc::clone(frames))
        })();
        match capture {
            Ok(capture) => Ok(Self {
                session,
                _capture: capture,
            }),
            Err(error) => {
                let _ = session.close();
                Err(error)
            }
        }
    }
}

impl Drop for LiveReviewer {
    fn drop(&mut self) {
        let _ = self.session.close();
    }
}

/// The script the `jev` command writes for the reviewer's stand-in classifier.
fn jev_script(frames: &Frames) -> PathBuf {
    frames.directory().join("jev-script.json")
}

/// Where the reviewer records the Explore prompts it sent, for the scripted
/// agent.
fn turns(frames: &Frames) -> PathBuf {
    frames.directory().join("turns")
}

struct VisionSession {
    // Close the viewer, then the terminal, before removing the private server
    // and repository.
    viewer: Option<viewer::ViewerPane>,
    live: Option<LiveReviewer>,
    agent: agent::ScriptedAgent,
    workspace: ReviewWorkspace,
    frames: Arc<Frames>,
    transcript: File,
    run: u64,
    action: u64,
}

impl VisionSession {
    fn start(options: &Options) -> Result<Self> {
        fs::create_dir_all(
            options
                .directory
                .parent()
                .context("output needs a parent directory")?,
        )?;
        fs::create_dir(&options.directory).context("vision output directory must be new")?;
        let frames = Arc::new(Frames::new(options.directory.clone())?);
        let transcript = File::create(options.directory.join("actions.jsonl"))?;
        let workspace = ReviewWorkspace::exploration(options.repository_type);
        let live = LiveReviewer::start(&workspace, &frames, 1)?;
        let session = Self {
            viewer: None,
            live: Some(live),
            agent: agent::ScriptedAgent::new(turns(&frames), workspace.mcp_port()),
            workspace,
            frames,
            transcript,
            run: 1,
            action: 0,
        };
        session.frames.wait(None, Duration::from_secs(10), true)?;
        fs::write(
            options.directory.join("session.json"),
            serde_json::to_vec_pretty(&json!({
                "repository": session.workspace.root(),
                "repository_type": options.repository_type.to_string(),
                "directory": options.directory,
                "stream": session.frames.stream_path(),
                "pid": std::process::id(),
            }))?,
        )?;
        Ok(session)
    }

    fn session(&self) -> Result<&Session> {
        Ok(&self
            .live
            .as_ref()
            .context("reviewer is not running; use reopen")?
            .session)
    }

    fn observe(&self, after: Option<u64>, timeout: Duration, settle: bool) -> Result<Value> {
        let frame = self.frames.wait(after, timeout, settle)?;
        let status = if after.is_some_and(|number| frame.number <= number) {
            ScreenStatus::Unchanged
        } else {
            ScreenStatus::Changed
        };
        self.report(&frame, status)
    }

    /// Wait for `text` to show, then report the screen that shows it.
    fn wait_for(&self, text: &str, timeout_ms: Option<u64>) -> Result<Value> {
        ensure!(!text.is_empty(), "wait text is empty");
        let timeout = timeout_ms.unwrap_or(5000);
        ensure!(timeout <= MAX_WAIT_MS, "maximum wait is {MAX_WAIT_MS} ms");
        let (frame, shown) = self
            .frames
            .wait_for_text(text, Duration::from_millis(timeout))?;
        self.report(
            &frame,
            if shown {
                ScreenStatus::Shown
            } else {
                ScreenStatus::Timeout
            },
        )
    }

    /// Describe `frame`, unless the reviewer has exited.
    fn report(&self, frame: &Frame, status: ScreenStatus) -> Result<Value> {
        let OperationResult::State(state) = self.session()?.execute(Operation::State)? else {
            bail!("expected terminal state");
        };
        let status = if state.exited.is_some() {
            ScreenStatus::Exited
        } else {
            status
        };
        Ok(json!({
            "status": status,
            "frame": frame,
            "frame_path": self.frames.path(frame.number),
            "repository": self.workspace.root(),
            "directory": self.frames.directory(),
            "exit_code": state.exited,
        }))
    }

    fn execute(&mut self, command: &Command) -> Result<Value> {
        let before = self.frames.latest().map_or(0, |frame| frame.number);
        self.action += 1;
        self.log(&json!({"action_id": self.action, "before": before, "command": command}))?;
        let result = self.apply(command, before);
        match &result {
            Ok(response) => self.log(&json!({"action_id": self.action, "response": response}))?,
            Err(error) => {
                self.log(&json!({"action_id": self.action, "error": format!("{error:#}")}))?;
            }
        }
        result
    }

    fn apply(&mut self, command: &Command, before: u64) -> Result<Value> {
        match command {
            Command::Observe { after, timeout_ms } => {
                let timeout = timeout_ms.unwrap_or(if after.is_some() { 1000 } else { 0 });
                ensure!(
                    timeout <= MAX_WAIT_MS,
                    "maximum observation wait is {MAX_WAIT_MS} ms"
                );
                return self.observe(*after, Duration::from_millis(timeout), false);
            }
            Command::Cells {
                x,
                y,
                width,
                height,
            } => {
                let frame = self.frames.latest()?;
                return Ok(json!({"status": "cells", "frame_number": frame.number,
                    "cells": frame.cells(*x, *y, *width, *height)?}));
            }
            Command::Note { .. } => {
                return Ok(json!({"status": "recorded", "frame_number": before}));
            }
            Command::Reopen => {
                self.live.take();
                // Closing drains the old recording. Only a frame captured after
                // that boundary can be an observation of the reopened process.
                let after_close = self.frames.latest()?.number;
                self.run += 1;
                self.live = Some(LiveReviewer::start(
                    &self.workspace,
                    &self.frames,
                    self.run,
                )?);
                return self.observe(Some(after_close), Duration::from_secs(10), true);
            }
            Command::Stop => {
                self.viewer.take();
                self.live.take();
                return Ok(json!({"status": "stopped", "frame_number": before}));
            }
            Command::Wait { text, timeout_ms } => return self.wait_for(text, *timeout_ms),
            Command::Turn { after, timeout_ms } => {
                let timeout = timeout_ms.unwrap_or(5000);
                ensure!(timeout <= MAX_WAIT_MS, "maximum wait is {MAX_WAIT_MS} ms");
                return self.agent.next_turn(*after, Duration::from_millis(timeout));
            }
            Command::Reply {
                turn,
                tool,
                arguments,
            } => {
                let result = self.agent.reply(*turn, tool, arguments.clone())?;
                let mut response = self.observe(Some(before), Duration::from_secs(2), true)?;
                response["reply"] = result;
                return Ok(response);
            }
            Command::Screenshot => return self.screenshot(),
            Command::ExplorePage => {
                let page = self.workspace.open_explore_page(self.frames.directory())?;
                return Ok(json!({"status": "opened", "frame_number": before, "page": page}));
            }
            Command::Jev { path, lines } => {
                fs::write(
                    jev_script(&self.frames),
                    serde_json::to_vec(
                        &json!({"insignificant": [{"path": path, "lines": lines}]}),
                    )?,
                )?;
                for key in ["r", "f"] {
                    self.session()?.execute(Operation::Key {
                        keys: vec![key.into()],
                        action: KeyAction::Press,
                    })?;
                }
                return self.await_jev(before);
            }
            _ => self.interact(command)?,
        }
        self.observe(Some(before), Duration::from_secs(1), true)
    }

    /// Show the live stream in a pane beside the driver's, when it runs in
    /// Herdr. A viewer that cannot open leaves the session headless.
    fn open_viewer(&mut self, options: &Options) {
        let (Some(placement), Some((herdr, pane))) = (options.viewer, viewer::Herdr::current())
        else {
            return;
        };
        let opened = (|| {
            let driver = std::env::current_exe()?
                .to_str()
                .context("driver path is not UTF-8")?
                .to_owned();
            let stream = self
                .frames
                .stream_path()
                .to_str()
                .context("stream path is not UTF-8")?
                .to_owned();
            viewer::ViewerPane::open(herdr, &pane, placement, SESSION_SIZE, |viewer| {
                vec![
                    driver,
                    "--view".into(),
                    stream,
                    "--close-pane".into(),
                    viewer.into(),
                ]
            })
        })();
        match opened {
            Ok(viewer) => self.viewer = Some(viewer),
            Err(error) => {
                let _ = self.log(&json!({"viewer_error": format!("{error:#}")}));
            }
        }
    }

    /// Save a PNG of the latest screen next to the text frames.
    fn screenshot(&self) -> Result<Value> {
        let frame = self.frames.latest()?.number;
        let directory = self.frames.directory().join("screenshots");
        fs::create_dir_all(&directory)?;
        let path = directory.join(format!("frame-{frame}.png"));
        let OperationResult::Screenshot(ScreenshotResult::Path(saved)) =
            self.session()?.execute(Operation::Screenshot {
                full: false,
                path: Some(
                    path.to_str()
                        .context("screenshot path is not UTF-8")?
                        .into(),
                ),
                zoom: None,
                background: None,
            })?
        else {
            bail!("expected a saved screenshot");
        };
        Ok(json!({"status": "screenshot", "frame_number": frame, "path": saved}))
    }

    /// Wait for `rf` to finish: it shows "Jev: classifying" first and marks
    /// later on a worker. A screen without either toast after two seconds is
    /// taken as `rf` refusing to run.
    fn await_jev(&self, before: u64) -> Result<Value> {
        let start = Instant::now();
        let mut after = before;
        loop {
            let remaining = Duration::from_secs(10).saturating_sub(start.elapsed());
            let response =
                self.observe(Some(after), remaining.min(Duration::from_secs(1)), true)?;
            let text = response["frame"]["text"].as_str().unwrap_or_default();
            let finished = text.contains("Jev: marked")
                || (!text.contains("Jev: classifying")
                    && start.elapsed() >= Duration::from_secs(2));
            if finished || remaining.is_zero() {
                return Ok(response);
            }
            after = response["frame"]["number"].as_u64().unwrap_or(after);
        }
    }

    fn interact(&self, command: &Command) -> Result<()> {
        let operation = match command {
            Command::Press { key } => Operation::Key {
                keys: vec![key.clone()],
                action: KeyAction::Press,
            },
            Command::Type { text } => {
                // Use the application's real paste handling, including newlines.
                Operation::Write {
                    data: format!("\x1b[200~{text}\x1b[201~"),
                }
            }
            Command::Click { x, y, text } => {
                match (x, y, text) {
                    (Some(x), Some(y), None) => {
                        let size = self.frames.latest()?.size;
                        ensure!(
                            *x < size.cols && *y < size.rows,
                            "click is outside the terminal"
                        );
                    }
                    (None, None, Some(text)) => ensure!(!text.is_empty(), "click text is empty"),
                    _ => bail!("click needs either x and y or text"),
                }
                Operation::Mouse {
                    action: MouseAction::Click {
                        x: *x,
                        y: *y,
                        on_text: text.clone(),
                        options: MouseOptions::default(),
                        clicks: 1,
                    },
                }
            }
            Command::Resize { cols, rows } => {
                ensure!(
                    (1..=300).contains(cols) && (1..=120).contains(rows),
                    "terminal size must be 1..300 columns and 1..120 rows"
                );
                Operation::Resize {
                    cols: *cols,
                    rows: *rows,
                }
            }
            _ => bail!("expected a terminal interaction"),
        };
        self.session()?.execute(operation)?;
        Ok(())
    }

    fn log(&mut self, value: &Value) -> Result<()> {
        serde_json::to_writer(&mut self.transcript, value)?;
        writeln!(self.transcript)?;
        self.transcript.flush()?;
        Ok(())
    }
}

struct Output {
    json: bool,
}

impl Output {
    fn response(&self, response: &Value) -> Result<()> {
        let mut output = io::stdout().lock();
        if self.json {
            writeln!(output, "{response}")?;
        } else if let Some(frame) = response.get("frame") {
            writeln!(
                output,
                "{} | frame {} | {}x{} | {}",
                response["status"],
                frame["number"],
                frame["size"]["cols"],
                frame["size"]["rows"],
                response["frame_path"]
            )?;
            writeln!(output, "{}", frame["text"].as_str().unwrap())?;
        } else {
            writeln!(output, "{}", serde_json::to_string_pretty(response)?)?;
        }
        output.flush()?;
        Ok(())
    }
}

/// Drive an isolated reviewer with one JSON command per stdin line.
/// Default output displays the actual terminal text; JSON output is optional.
pub fn run(options: &Options) -> Result<()> {
    let input = Input::start(options.commands.clone())?;
    let mut session = VisionSession::start(options)?;
    session.open_viewer(options);
    let output = Output { json: options.json };
    let initial = session.observe(None, Duration::ZERO, false)?;
    session.log(&json!({"started": initial}))?;
    output.response(&initial)?;
    let idle = loop {
        let line = match input.next(options.stop_after_idle)? {
            Next::Line(line) => line,
            Next::Ended => break false,
            Next::Idle => break true,
        };
        let command = (|| -> Result<Command> { Ok(serde_json::from_str::<Command>(&line?)?) })();
        let stop = command
            .as_ref()
            .is_ok_and(|command| matches!(command, Command::Stop));
        let response = command
            .and_then(|command| session.execute(&command))
            .unwrap_or_else(|error| json!({"status": "error", "error": format!("{error:#}")}));
        output.response(&response)?;
        if stop {
            return Ok(());
        }
    };
    let mut stopped = session.execute(&Command::Stop)?;
    if idle {
        stopped["reason"] = "idle".into();
    }
    output.response(&stopped)?;
    Ok(())
}
