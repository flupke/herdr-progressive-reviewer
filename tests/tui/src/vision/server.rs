//! The vision MCP server: an agent's eyes and hands on the reviewer, running in the user's
//! Herdr. Every tool that acts returns the screen the reviewer painted in reaction, and every
//! mistake is an error.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use herdr_client::client::HerdrClient;
use review_repository::repository::RepoType;
use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::scratch::Seed;
use super::screen::{Cell, Screen};
use super::session::{Input, Session, StartOptions};

/// The longest a `wait_for` or `turn` may wait.
const MAX_WAIT_MS: u64 = 60_000;

const INSTRUCTIONS: &str = "Eyes and hands on the reviewer's terminal pane. `start` builds the reviewer from this checkout and runs it in a new workspace of the user's Herdr, named \"reviewer vision\", on a scratch repository, beside a stand-in implementation agent; the user can watch it there. Every action (key, type, click, jev, reply) returns the screen once the reviewer has read the input and painted its reaction, not after a delay. For asynchronous work, call wait_for with the text the finished screen shows. Coordinates are zero-based terminal cells. Use screenshot to judge how the screen looks, screen with styled for colors as ANSI sequences, and note to record what you checked and found in the session's actions.jsonl. Call stop when done: it closes the workspace and removes the scratch repository.";

/// The repository a session reviews.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum RepositoryKind {
    #[default]
    Jj,
    Git,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct StartInput {
    /// The scratch repository's version control, jj by default.
    #[serde(default)]
    repository: RepositoryKind,
    /// Files of the base change, by repository-relative path. With neither base nor change, a
    /// sample change (a renamed function, a Rust file, a Markdown file with Unicode and a long
    /// line) is reviewed.
    #[serde(default)]
    base: BTreeMap<String, String>,
    /// Files written on top of the base: the change under review.
    #[serde(default)]
    change: BTreeMap<String, String>,
    /// Show the session's workspace in Herdr at once, taking the user's focus. False by default:
    /// the user switches to the "reviewer vision" workspace to watch.
    #[serde(default)]
    focus: bool,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ScreenInput {
    /// Also return the screen with its colors and styles as ANSI SGR sequences, to tell focus,
    /// selection or dimmed text apart.
    #[serde(default)]
    styled: bool,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct KeyInput {
    /// Keys pressed in order, as Herdr names them: a character (`a`, `?`, `A` for shift+a),
    /// `enter`, `esc`, `tab`, `backspace`, `space`, `up`, `down`, `left`, `right`, `f1`..`f12`,
    /// `pageup`, `pagedown`, `home`, `end`, `insert`, `delete`, with modifiers joined by `+`
    /// (`ctrl+enter`, `shift+tab`, `alt+x`). An unknown name is an error.
    keys: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct TypeInput {
    /// The text to paste into the focused editor or field, newlines included, as a terminal
    /// pastes it (bracketed paste).
    text: String,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ClickInput {
    /// The zero-based column of the cell to click, with `y`.
    x: Option<u16>,
    /// The zero-based row of the cell to click, with `x`.
    y: Option<u16>,
    /// Instead of `x` and `y`: click the first cell where the screen shows this text. Clicking by
    /// text needs no counting of columns across wide characters.
    text: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct WaitInput {
    /// The text to wait for.
    text: String,
    /// Wait until the screen no longer shows the text, instead of until it does.
    #[serde(default)]
    absent: bool,
    /// The longest wait, 5000 ms by default, at most 60000. The wait ends as soon as a painted
    /// frame satisfies it; running out is an error that shows the latest screen.
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct JevInput {
    /// The repository-relative file whose hunks the stand-in Jev classifies as insignificant.
    path: String,
    /// One-based lines: each diff hunk of `path` that adds one of them (current numbering) or
    /// removes one (base numbering) is insignificant.
    lines: Vec<u32>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct TurnInput {
    /// Return the first turn after this number, instead of after the last turn returned.
    after: Option<u64>,
    /// The longest wait for the reviewer to send the turn, 15000 ms by default, at most 60000.
    timeout_ms: Option<u64>,
}

/// A tool of the reviewer's MCP endpoint that the stand-in agent calls.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AgentTool {
    SubmitQuestion,
    SubmitConclusion,
    Reply,
    GetThread,
    ListThreads,
    GetNewMessages,
}

impl AgentTool {
    fn name(self) -> &'static str {
        match self {
            Self::SubmitQuestion => "submit_question",
            Self::SubmitConclusion => "submit_conclusion",
            Self::Reply => "reply",
            Self::GetThread => "get_thread",
            Self::ListThreads => "list_threads",
            Self::GetNewMessages => "get_new_messages",
        }
    }
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReplyInput {
    /// The turn to answer, which must be the latest one.
    turn: u64,
    /// The reviewer's MCP tool to call as the stand-in agent.
    tool: AgentTool,
    /// The tool's arguments, as the reviewer's MCP tool takes them (examples in
    /// tests/tui/examples). The turn's access value (`review`), and its `instance`, `request` and
    /// `checkpoint` (inside `update` for `submit_question`) are filled in unless given; an
    /// `interpretation` without an `answer` is about the answer the turn brought. Arguments with
    /// their own `review` are sent as they are, to check how the reviewer rejects them.
    arguments: serde_json::Map<String, Value>,
}

/// What a note records.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum NoteKind {
    Checked,
    Finding,
    Untested,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoteInput {
    kind: NoteKind,
    /// For a finding: the expected behavior, the observed behavior, and the steps to reproduce.
    text: String,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoInput {}

/// Where a session's reviewer comes from, and where its files go.
#[derive(Clone)]
pub struct Setup {
    /// The Herdr the sessions run in.
    pub herdr_socket: PathBuf,
    /// The checkout whose reviewer `start` builds.
    pub checkout: PathBuf,
    /// The directory under which each session gets a new directory.
    pub sessions: PathBuf,
}

impl Setup {
    /// The user's Herdr, this checkout, and `tests/tui/target/vision`.
    pub fn from_env() -> Result<Self> {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        Ok(Self {
            herdr_socket: std::env::var_os("HERDR_SOCKET_PATH")
                .context(
                    "HERDR_SOCKET_PATH is not set: run the vision server from an agent inside Herdr",
                )?
                .into(),
            checkout: manifest.join("../.."),
            sessions: manifest.join("target/vision"),
        })
    }
}

#[derive(Clone)]
pub(crate) struct Server {
    setup: Setup,
    session: Arc<Mutex<Option<Session>>>,
}

/// What a tool returns: a summary, and the screen's text when it has one.
struct Outcome {
    summary: Value,
    screen: Option<Screen>,
    image: Option<String>,
}

impl Outcome {
    fn screen(status: &str, screen: Screen) -> Self {
        Self {
            summary: json!({"status": status}),
            screen: Some(screen),
            image: None,
        }
    }

    fn with(mut self, key: &str, value: impl Serialize) -> Self {
        self.summary[key] = json!(value);
        self
    }

    fn into_result(mut self) -> CallToolResult {
        let mut content = Vec::new();
        if let Some(screen) = &self.screen {
            self.summary["frame"] = json!(screen.frame);
            self.summary["size"] = json!({"columns": screen.columns, "rows": screen.rows});
        }
        content.push(ContentBlock::text(self.summary.to_string()));
        if let Some(screen) = self.screen {
            content.push(ContentBlock::text(screen.text));
            if let Some(styled) = screen.styled {
                content.push(ContentBlock::text(styled));
            }
        }
        if let Some(image) = self.image {
            content.push(ContentBlock::image(image, "image/png"));
        }
        CallToolResult::success(content)
    }
}

#[tool_router]
impl Server {
    pub(crate) fn new(setup: Setup) -> Self {
        Self {
            setup,
            session: Arc::default(),
        }
    }

    /// Run `action` on the session off the async runtime, record it in the session's
    /// transcript, and turn an error into an error result the agent reads.
    async fn run(
        &self,
        tool: &'static str,
        arguments: Value,
        action: impl FnOnce(&Setup, &mut Option<Session>) -> Result<Outcome> + Send + 'static,
    ) -> Result<CallToolResult, ErrorData> {
        let setup = self.setup.clone();
        let session = Arc::clone(&self.session);
        tokio::task::spawn_blocking(move || {
            let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
            let outcome = action(&setup, &mut session);
            if let Some(session) = session.as_ref() {
                record(session, tool, &arguments, &outcome);
            }
            match outcome {
                Ok(outcome) => outcome.into_result(),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(format!("{error:#}"))]),
            }
        })
        .await
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))
    }

    #[tool(
        description = "Start a vision session: build the reviewer from this checkout, create a scratch repository with a base change and a change under review, and run the reviewer in a new \"reviewer vision\" workspace of the user's Herdr beside a stand-in implementation agent (a script named claude that Explore prompts). Returns the reviewer's first screen, which may come before the repository has loaded (wait_for a file name), the workspace and pane IDs, the repository (edit its files to exercise filesystem-driven updates) and the session directory. One session at a time."
    )]
    async fn start(
        &self,
        Parameters(input): Parameters<StartInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let arguments = json!(input);
        self.run("start", arguments, move |setup, session| {
            ensure!(session.is_none(), "a session is running; stop it first");
            let reviewer = build_reviewer(&setup.checkout)?;
            let seed = if input.base.is_empty() && input.change.is_empty() {
                Seed::sample()
            } else {
                Seed {
                    base: input.base,
                    change: input.change,
                }
            };
            let options = StartOptions {
                repository: match input.repository {
                    RepositoryKind::Jj => RepoType::Jj,
                    RepositoryKind::Git => RepoType::Git,
                },
                seed,
                focus: input.focus,
            };
            let directory = setup.sessions.join(format!(
                "{}-{}",
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
                std::process::id()
            ));
            let (started, screen) = Session::start(
                &HerdrClient::new(
                    setup.herdr_socket.clone(),
                    "herdr.progressive-reviewer".into(),
                    setup.sessions.clone(),
                ),
                reviewer,
                &options,
                directory,
            )?;
            let outcome = Outcome::screen("started", screen).with("session", started.describe());
            *session = Some(started);
            Ok(outcome)
        })
        .await
    }

    #[tool(
        description = "Read the reviewer's screen as it is now, one line per row: what the user sees in the pane. With styled, also its colors and styles as ANSI SGR sequences."
    )]
    async fn screen(
        &self,
        Parameters(input): Parameters<ScreenInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run("screen", json!(input), move |_, session| {
            Ok(Outcome::screen(
                "screen",
                running(session)?.screen(input.styled)?,
            ))
        })
        .await
    }

    #[tool(
        description = "Press keys in the reviewer, in order, and return the screen once the reviewer has read them and painted its reaction."
    )]
    async fn key(
        &self,
        Parameters(input): Parameters<KeyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run("key", json!(input), move |_, session| {
            Ok(Outcome::screen(
                "reacted",
                running(session)?.act(&Input::Keys(&input.keys))?,
            ))
        })
        .await
    }

    #[tool(
        name = "type",
        description = "Paste text into the reviewer as a terminal pastes it, newlines included: it goes to the focused editor or field. Returns the screen once the reviewer has read it and painted its reaction."
    )]
    async fn type_text(
        &self,
        Parameters(input): Parameters<TypeInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run("type", json!(input), move |_, session| {
            Ok(Outcome::screen(
                "reacted",
                running(session)?.act(&Input::Text(&input.text))?,
            ))
        })
        .await
    }

    #[tool(
        description = "Left-click a cell of the reviewer, given as x and y (zero-based column and row), or as text: the first cell where the screen shows it. Returns the clicked cell and the screen once the reviewer has painted its reaction. Text the screen does not show, or a cell outside it, is an error."
    )]
    async fn click(
        &self,
        Parameters(input): Parameters<ClickInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run("click", json!(input), move |_, session| {
            let session = running(session)?;
            let cell = match (input.x, input.y, input.text) {
                (Some(x), Some(y), None) => Cell { x, y },
                (None, None, Some(text)) => session.screen(false)?.locate(&text)?,
                _ => bail!("click takes either x and y, or text"),
            };
            Ok(Outcome::screen("reacted", session.act(&Input::Click(cell))?).with("cell", cell))
        })
        .await
    }

    #[tool(
        description = "Wait until the reviewer's screen shows a text, or no longer shows it with absent, and return that screen. The screen is checked at once, then after each frame the reviewer paints: use it for asynchronous work (a refresh, Jev, an agent's turn), with text the screen before did not show."
    )]
    async fn wait_for(
        &self,
        Parameters(input): Parameters<WaitInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run("wait_for", json!(input), move |_, session| {
            let guard = wait_guard(input.timeout_ms, 5000)?;
            let screen = running(session)?.wait_for(&input.text, input.absent, guard)?;
            Ok(Outcome::screen(
                if input.absent { "gone" } else { "shown" },
                screen,
            ))
        })
        .await
    }

    #[tool(
        description = "Save a PNG of the reviewer's screen, rendered with a bundled JetBrains Mono, and return it as an image with its path in the session directory: to judge how the screen looks (spacing, alignment, theme). Read the text screens for everything else. A character no outline font of this machine draws (a color emoji) is drawn as ? in its cells, and listed as drawn_as_question_marks."
    )]
    async fn screenshot(
        &self,
        Parameters(_): Parameters<NoInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run("screenshot", json!({}), move |_, session| {
            let (screen, saved) = running(session)?.screenshot()?;
            let png = std::fs::read(&saved.path)?;
            let mut outcome = Outcome::screen("screenshot", screen).with("path", &saved.path);
            if !saved.replaced.is_empty() {
                outcome = outcome.with("drawn_as_question_marks", &saved.replaced);
            }
            outcome.image = Some(base64::engine::general_purpose::STANDARD.encode(png));
            Ok(outcome)
        })
        .await
    }

    #[tool(
        description = "Stand in for the paid Jev classifier, which vision sessions never reach: classify as insignificant every diff hunk of path that adds one of lines (current numbering) or removes one (base numbering), press rf, and return the screen once Jev has finished, with its result (\"Jev: marked\"), or the screen of rf refusing. Nearby edits share one diff hunk, as they do for Jev. The script is kept as jev-script.json in the session directory: write it yourself before an Explore round to have Jev mark at the round's start."
    )]
    async fn jev(
        &self,
        Parameters(input): Parameters<JevInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run("jev", json!(input), move |_, session| {
            Ok(Outcome::screen(
                "classified",
                running(session)?.jev(&input.path, &input.lines)?,
            ))
        })
        .await
    }

    #[tool(
        description = "Wait for the next Explore prompt the reviewer sent to its agent, and return it as a turn: its number, kind (kickoff, wakeup, implement), text, and the access value and identity a reply needs. Each call returns the turn after the last one returned, or after `after`. The reviewer records turns only in a vision session."
    )]
    async fn turn(
        &self,
        Parameters(input): Parameters<TurnInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run("turn", json!(input), move |_, session| {
            let guard = wait_guard(input.timeout_ms, 15_000)?;
            let turn = running(session)?.next_turn(input.after, guard)?;
            Ok(Outcome {
                summary: json!({"status": "turn", "turn": turn}),
                screen: None,
                image: None,
            })
        })
        .await
    }

    #[tool(
        description = "Answer the latest turn as the stand-in agent: call one of the reviewer's MCP tools on its real endpoint. Returns the tool's result as `reply`, and the screen once the reviewer has shown it. A turn that is not the latest is an error, so a script never answers a prompt the reviewer replaced; the reviewer's own validation errors come back in `reply`."
    )]
    async fn reply(
        &self,
        Parameters(input): Parameters<ReplyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run("reply", json!(input), move |_, session| {
            let (reply, screen) = running(session)?.reply(
                input.turn,
                input.tool.name(),
                Value::Object(input.arguments),
            )?;
            Ok(Outcome::screen("replied", screen).with("reply", reply))
        })
        .await
    }

    #[tool(
        description = "Run the Herdr action that opens the Explore page, as Herdr runs it in the session's workspace, with a browser that records the address instead of opening it. Returns that address, with its token, as page: open it with curl (curl -sL -c jar -b jar \"$PAGE\") or a browser on this machine."
    )]
    async fn explore_page(
        &self,
        Parameters(_): Parameters<NoInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run("explore_page", json!({}), move |_, session| {
            let page = running(session)?.explore_page()?;
            Ok(Outcome {
                summary: json!({"status": "opened", "page": page}),
                screen: None,
                image: None,
            })
        })
        .await
    }

    #[tool(
        description = "Record what you checked, a finding (expected behavior, observed behavior, steps to reproduce), or an area left untested, in the session's actions.jsonl."
    )]
    async fn note(
        &self,
        Parameters(input): Parameters<NoteInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run("note", json!(input), move |_, session| {
            let directory = running(session)?.directory().to_owned();
            Ok(Outcome {
                summary: json!({"status": "recorded", "transcript": directory.join("actions.jsonl")}),
                screen: None,
                image: None,
            })
        })
        .await
    }

    #[tool(
        description = "Start the reviewer and its stand-in agent again, in new panes of the session's tab, on the same repository and state, and return the reviewer's first screen and the new pane IDs."
    )]
    async fn reopen(
        &self,
        Parameters(_): Parameters<NoInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run("reopen", json!({}), move |_, session| {
            let session = running(session)?;
            let screen = session.reopen()?;
            Ok(Outcome::screen("reopened", screen).with("session", session.describe()))
        })
        .await
    }

    #[tool(
        description = "Stop the session: close its Herdr workspace with the reviewer and the stand-in agent, and remove the scratch repository. A workspace that is no longer the session's alone (renamed, or holding a pane the session did not open) stays open, and the result says why. The session directory, with actions.jsonl and the screenshots, stays."
    )]
    async fn stop(&self, Parameters(_): Parameters<NoInput>) -> Result<CallToolResult, ErrorData> {
        self.run("stop", json!({}), move |_, session| {
            let stopped = session.take().context("no session is running")?;
            let directory = stopped.directory().to_owned();
            let transcript = directory.join("actions.jsonl");
            let workspace = stopped.stop().describe();
            let outcome = Outcome {
                summary: json!({"status": "stopped", "workspace": workspace, "directory": directory}),
                screen: None,
                image: None,
            };
            append(&transcript, "stop", &json!({}), &Ok(&outcome));
            Ok(outcome)
        })
        .await
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(INSTRUCTIONS)
    }
}

fn running(session: &mut Option<Session>) -> Result<&mut Session> {
    session
        .as_mut()
        .context("no session is running; call start first")
}

fn wait_guard(timeout_ms: Option<u64>, default: u64) -> Result<Duration> {
    let timeout = timeout_ms.unwrap_or(default);
    ensure!(
        timeout <= MAX_WAIT_MS,
        "timeout_ms is at most {MAX_WAIT_MS}"
    );
    Ok(Duration::from_millis(timeout))
}

/// Append one tool call to the session's `actions.jsonl`.
fn record(session: &Session, tool: &str, arguments: &Value, outcome: &Result<Outcome>) {
    append(
        &session.directory().join("actions.jsonl"),
        tool,
        arguments,
        &outcome.as_ref().map_err(|error| format!("{error:#}")),
    );
}

fn append(
    transcript: &std::path::Path,
    tool: &str,
    arguments: &Value,
    outcome: &std::result::Result<&Outcome, String>,
) {
    let entry = match outcome {
        Ok(outcome) => json!({
            "tool": tool, "arguments": arguments, "result": outcome.summary,
            "frame": outcome.screen.as_ref().map(|screen| screen.frame),
            "screen": outcome.screen.as_ref().map(|screen| &screen.text),
        }),
        Err(error) => json!({"tool": tool, "arguments": arguments, "error": error}),
    };
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(transcript)
    {
        let _ = writeln!(file, "{entry}");
    }
}

/// Build the reviewer of `checkout`, as the session runs it, and return its path. The build
/// runs in the checkout's dev shell, as `make` does, since the server runs outside it.
fn build_reviewer(checkout: &std::path::Path) -> Result<PathBuf> {
    let output = std::process::Command::new(checkout.join("scripts/dev-shell"))
        .args([
            "cargo",
            "build",
            "--locked",
            "-p",
            "reviewer",
            "--bin",
            "reviewer",
            "--bin",
            "reviewer-control",
            "--message-format",
            "short",
        ])
        .current_dir(checkout)
        .output()
        .context("cannot run cargo in the dev shell")?;
    if !output.status.success() {
        bail!(
            "the reviewer does not build:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| checkout.join("target"), PathBuf::from);
    Ok(target.join("debug/reviewer"))
}

/// Serve the vision tools on standard input and output until the client leaves or a signal
/// stops the server; a running session stops then.
pub fn serve(setup: Setup) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let server = Server::new(setup);
    let session = Arc::clone(&server.session);
    let outcome = runtime.block_on(async move {
        use tokio::signal::unix::{SignalKind, signal};
        let running = server.serve(rmcp::transport::stdio()).await?;
        let mut terminate = signal(SignalKind::terminate())?;
        let mut hangup = signal(SignalKind::hangup())?;
        let mut interrupt = signal(SignalKind::interrupt())?;
        tokio::select! {
            waited = running.waiting() => {
                waited?;
            }
            _ = terminate.recv() => {}
            _ = hangup.recv() => {}
            _ = interrupt.recv() => {}
        }
        anyhow::Ok(())
    });
    session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    outcome
}
