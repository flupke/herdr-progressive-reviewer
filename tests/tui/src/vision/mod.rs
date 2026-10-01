mod capture;
mod command;
mod frame;
mod input;

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use review_repository::repository::RepoType;
use serde_json::{Value, json};
use tui_test::{
    KeyAction, MouseAction, MouseOptions, Operation, OperationResult, ScreenshotResult, Session,
};

use crate::fixture::ReviewWorkspace;
use capture::Capture;
use command::Command;
use frame::Frames;
use input::Input;

/// Options for a persistent, isolated terminal exploration session.
pub struct Options {
    pub repository_type: RepoType,
    pub directory: PathBuf,
    pub json: bool,
    /// A named pipe to read commands from instead of standard input. Each
    /// writer may close it; the driver reopens it for the next one.
    pub commands: Option<PathBuf>,
}

struct LiveReviewer {
    session: Session,
    _capture: Capture,
}

impl LiveReviewer {
    fn start(workspace: &ReviewWorkspace, frames: &Arc<Frames>, run: u64) -> Result<Self> {
        let session = workspace.open_vision(
            &frames.directory().join(format!("recording-{run}")),
            &jev_script(frames),
        )?;
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

struct VisionSession {
    // Close the terminal before removing the private server and repository.
    live: Option<LiveReviewer>,
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
            live: Some(live),
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
        let OperationResult::State(state) = self.session()?.execute(Operation::State)? else {
            bail!("expected terminal state");
        };
        let status = if state.exited.is_some() {
            "exited"
        } else if after.is_some_and(|number| frame.number <= number) {
            "unchanged"
        } else {
            "changed"
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
                ensure!(timeout <= 30_000, "maximum observation wait is 30000 ms");
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
                self.live.take();
                return Ok(json!({"status": "stopped", "frame_number": before}));
            }
            Command::Screenshot => return self.screenshot(),
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
    let output = Output { json: options.json };
    let initial = session.observe(None, Duration::ZERO, false)?;
    session.log(&json!({"started": initial}))?;
    output.response(&initial)?;
    while let Some(line) = input.events.recv()? {
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
    }
    output.response(&session.execute(&Command::Stop)?)?;
    Ok(())
}
