//! A Codex TUI in a Herdr test server, on an app server of its own with a scratch Codex home and
//! a local model fixture: the reviewer learns the agent's session from its call to a reviewer
//! tool, then follows the session through the daemon, across a `/resume` and a restart of the
//! daemon. It calls no model API.

use std::fs;
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use codex_daemon::{Daemon, NoDaemon, SessionEvent, SessionStatus, TurnEnd};
use herdr_client::protocol::{AgentPort as _, AgentStatus, HerdrEvent, PaneId};
use review_mcp::{Endpoint, Response, Server};
use review_mcp_config::{Client, UserConfig};
use review_test_support::{GUARD, HerdrTestServer, TestPort};

#[test]
fn the_reviewer_follows_a_codex_agent_s_session_through_its_daemon() {
    let codex = CodexHome::new();
    let port = TestPort::new();
    let endpoint = Endpoint::for_repository(&codex.workspace(), Some(port.number())).unwrap();
    let (named, sessions) = mpsc::channel();
    let _reviewer = Server::start(endpoint, move |request| {
        let _ = named.send(request.agent_session.clone());
        request.respond(Ok(Response::Threads(Vec::new())));
        Ok(())
    })
    .unwrap();
    let provider = Provider::start(&codex);
    let mut daemon = Some(AppServer::start(&codex, &provider, &port));
    let herdr = HerdrTestServer::start(&codex.workspace());
    let pane = codex.start_tui(&herdr, &["--remote", "unix://"]);

    prompt(&herdr, &pane, "call the reviewer");
    let session = sessions
        .recv_timeout(GUARD)
        .expect("the agent called no reviewer tool")
        .expect("the call named no agent session");
    let processes = herdr.client().pane_process_info(&pane).unwrap();
    let (heard, events) = mpsc::channel();
    let _follow = Daemon::of_pane(&processes.foreground_processes)
        .unwrap()
        .follow(&session, Duration::from_millis(20), move |event| {
            let _ = heard.send(event);
        });
    let mut events = Events(events);
    let joined = events.until("the join", |event| matches!(event, SessionEvent::Joined(_)));
    if joined.last() == Some(&SessionEvent::Joined(Some(SessionStatus::Active))) {
        events.until("the end of the first turn", |event| {
            matches!(event, SessionEvent::TurnEnded { .. })
        });
    }

    prompt(&herdr, &pane, "second turn");
    let heard = events.turn("second turn");
    assert!(
        !heard.contains(&SessionEvent::Resumed),
        "the reviewer's own join is no resume: {heard:?}"
    );

    prompt(&herdr, &pane, "/new");
    prompt(&herdr, &pane, &format!("/resume {session}"));
    let typed = Instant::now();
    events.until("the resume", |event| *event == SessionEvent::Resumed);
    assert!(
        typed.elapsed() < Duration::from_secs(1),
        "the resume was heard {:?} after it was typed",
        typed.elapsed()
    );

    prompt(&herdr, &pane, "after the resume");
    events.turn("after the resume");

    daemon.take().unwrap().stop();
    events.until("the loss of the daemon", |event| {
        matches!(event, SessionEvent::Lost(_))
    });
    herdr.wait_output(&pane, "Reconnecting to server");
    daemon = Some(AppServer::start(&codex, &provider, &port));
    let rejoined = events.until("the join again", |event| {
        matches!(event, SessionEvent::Joined(_))
    });
    assert_eq!(
        rejoined
            .iter()
            .filter(|event| matches!(event, SessionEvent::Lost(_)))
            .count(),
        0,
        "the loss is reported once: {rejoined:?}"
    );
    herdr.wait_output(&pane, "? for shortcuts");
    prompt(&herdr, &pane, "after the restart");
    events.turn("after the restart");
    drop(daemon);
}

#[test]
fn a_codex_agent_without_the_daemon_has_none_to_join() {
    let codex = CodexHome::new();
    // The agent takes no turn: its provider listens nowhere.
    codex.configure(9);
    let herdr = HerdrTestServer::start(&codex.workspace());
    let pane = codex.start_tui(&herdr, &["--no-daemon"]);

    let processes = herdr.client().pane_process_info(&pane).unwrap();

    assert_eq!(
        Daemon::of_pane(&processes.foreground_processes),
        Err(NoDaemon::WithoutDaemon)
    );
}

/// What the reviewer heard of the session, in order.
struct Events(mpsc::Receiver<SessionEvent>);

impl Events {
    /// The events up to the first one that `last` accepts, that one included.
    fn until(
        &mut self,
        what: &str,
        mut last: impl FnMut(&SessionEvent) -> bool,
    ) -> Vec<SessionEvent> {
        let mut heard = Vec::new();
        loop {
            let event = self
                .0
                .recv_timeout(GUARD)
                .unwrap_or_else(|_| panic!("the reviewer did not hear {what}, only {heard:?}"));
            let done = last(&event);
            heard.push(event);
            if done {
                return heard;
            }
        }
    }

    /// The events up to the end of the turn that starts on `prompt`, once they say that it
    /// started, took the prompt and completed, in that order.
    fn turn(&mut self, prompt: &str) -> Vec<SessionEvent> {
        let mut turn = None;
        let heard = self.until(&format!("the turn of {prompt:?}"), |event| match event {
            SessionEvent::Prompt { turn: of, text } if text == prompt => {
                turn = Some(of.clone());
                false
            }
            SessionEvent::TurnEnded { turn: ended, .. } => turn.as_ref() == Some(ended),
            _ => false,
        });
        let turn = turn.unwrap();
        let of_turn: Vec<_> = heard
            .iter()
            .filter(|event| match event {
                SessionEvent::TurnStarted { turn: of }
                | SessionEvent::Prompt { turn: of, .. }
                | SessionEvent::TurnEnded { turn: of, .. } => *of == turn,
                _ => false,
            })
            .cloned()
            .collect();
        assert_eq!(
            of_turn,
            [
                SessionEvent::TurnStarted { turn: turn.clone() },
                SessionEvent::Prompt {
                    turn: turn.clone(),
                    text: prompt.to_owned(),
                },
                SessionEvent::TurnEnded {
                    turn,
                    end: TurnEnd::Completed,
                },
            ]
        );
        heard
    }
}

/// Submits `text` to the agent of `pane`, as the reviewer and the user do.
fn prompt(herdr: &HerdrTestServer, pane: &PaneId, text: &str) {
    herdr.run_cli(&["agent", "prompt", &pane.0, text]);
}

trait WaitOutput {
    /// Waits until the pane `pane` shows `text`, as Herdr reads it.
    fn wait_output(&self, pane: &PaneId, text: &str);
}

impl WaitOutput for HerdrTestServer {
    fn wait_output(&self, pane: &PaneId, text: &str) {
        let guard = GUARD.as_millis().to_string();
        self.run_cli(&[
            "pane",
            "wait-output",
            &pane.0,
            "--match",
            text,
            "--source",
            "visible",
            "--timeout",
            &guard,
        ]);
    }
}

/// A scratch Codex home, with its user's home and the workspace the agent works in.
struct CodexHome {
    directory: tempfile::TempDir,
    codex: PathBuf,
}

impl CodexHome {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        for name in ["workspace", "codex", "home"] {
            fs::create_dir(directory.path().join(name)).unwrap();
        }
        let home = Self {
            directory,
            codex: native_codex(),
        };
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .arg(home.workspace())
                .status()
                .unwrap()
                .success()
        );
        home
    }

    fn workspace(&self) -> PathBuf {
        self.directory.path().join("workspace")
    }

    fn home(&self) -> PathBuf {
        self.directory.path().join("codex")
    }

    /// The variables every Codex process of this home runs with.
    fn environment(&self) -> [(&'static str, PathBuf); 2] {
        [
            ("CODEX_HOME", self.home()),
            ("HOME", self.directory.path().join("home")),
        ]
    }

    /// Writes the configuration that has Codex use `provider`, trust the workspace, and reach
    /// the reviewer through the bridge, which may list the threads without asking.
    fn configure(&self, provider: u16) {
        let config = self.home().join("config.toml");
        let _ = fs::remove_file(&config);
        assert!(
            UserConfig::new(
                Client::Codex,
                &self.home(),
                env!("CARGO_BIN_EXE_reviewer-mcp").into()
            )
            .install()
            .unwrap()
        );
        let registration = fs::read_to_string(&config).unwrap();
        let workspace = serde_json::to_string(&self.workspace()).unwrap();
        fs::write(
            &config,
            format!(
                // A model with no migration: the TUI would announce one, and drop what is typed
                // meanwhile.
                r#"model = "gpt-6.1-sol"
model_provider = "fixture"
approval_policy = "never"
sandbox_mode = "read-only"
check_for_update_on_startup = false

[model_providers.fixture]
name = "Private test provider"
base_url = "http://127.0.0.1:{provider}/v1"
wire_api = "responses"
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0

[projects.{workspace}]
trust_level = "trusted"

[tui]
screen_reader_detection_done = true

{registration}
[mcp_servers.herdr_reviewer.tools.list_threads]
approval_mode = "approve"
"#
            ),
        )
        .unwrap();
    }

    /// Starts the Codex TUI with `arguments` in a pane of `herdr`, and returns once Herdr
    /// reports it idle: it takes what is typed from then on.
    fn start_tui(&self, herdr: &HerdrTestServer, arguments: &[&str]) -> PaneId {
        let workspace = herdr.run_cli_json(&[
            "workspace",
            "create",
            "--cwd",
            &self.workspace().to_string_lossy(),
            "--label",
            "codex",
            "--no-focus",
        ]);
        let pane = PaneId(
            workspace["result"]["root_pane"]["pane_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
        let events = herdr.events();
        let command = self
            .environment()
            .iter()
            .map(|(name, value)| format!("{name}={}", quoted(&value.to_string_lossy())))
            .chain(std::iter::once(quoted(&self.codex.to_string_lossy())))
            .chain(arguments.iter().map(|argument| quoted(argument)))
            .collect::<Vec<_>>()
            .join(" ");
        herdr.run_cli(&["pane", "run", &pane.0, &format!("env {command}")]);
        events.wait_for("the Codex agent", |event| {
            matches!(event, HerdrEvent::AgentDetected { pane_id, agent: Some(agent), released: false, .. }
                if *pane_id == pane && agent == "codex")
        });
        herdr.agent_statuses(&pane).wait_for(AgentStatus::Idle);
        pane
    }
}

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The native Codex binary of the `codex` on the `PATH`, which the npm package runs under a
/// Node wrapper: the test signals the app server itself.
fn native_codex() -> PathBuf {
    let codex = std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|directory| directory.join("codex"))
                .find(|candidate| candidate.is_file())
        })
        .expect("Codex is not installed")
        .canonicalize()
        .unwrap();
    let package = codex.parent().and_then(Path::parent).unwrap();
    let native = fs::read_dir(package.join("node_modules/@openai"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("codex-linux-")
        })
        .flat_map(|entry| {
            fs::read_dir(entry.path().join("vendor"))
                .into_iter()
                .flatten()
        })
        .flatten()
        .map(|target| target.path().join("bin/codex"))
        .find(|binary| binary.is_file());
    native.unwrap_or(codex)
}

/// The local model fixture.
struct Provider {
    port: u16,
    _child: Lifetime,
}

impl Provider {
    fn start(codex: &CodexHome) -> Self {
        let mut child = Command::new("python3")
            .arg("-B")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/codex_provider.py"
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(fs::File::create(codex.directory.path().join("provider.log")).unwrap())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = line.trim().parse().expect("the provider printed no port");
        codex.configure(port);
        Self {
            port,
            _child: Lifetime::new(child),
        }
    }
}

/// An unmanaged Codex app server on the home's control socket, which the TUI takes for its
/// daemon: unlike the managed daemon, it never updates itself.
struct AppServer {
    pid: String,
    lifetime: Lifetime,
}

/// What the app server writes once its control socket accepts connections.
const LISTENING: &str = "app-server control socket listening";

impl AppServer {
    fn start(codex: &CodexHome, provider: &Provider, port: &TestPort) -> Self {
        assert_ne!(provider.port, 0);
        // The shell stops the server once the test lets go of its stdin, even by dying.
        let mut child = Command::new("sh")
            .args([
                "-c",
                r#"exec 3<&0; "$@" </dev/null >/dev/null & server=$!; echo "$server"
                   { cat <&3 >/dev/null; kill -TERM "$server" 2>/dev/null; } & wait "$server""#,
                "sh",
            ])
            .arg(&codex.codex)
            .args(["app-server", "--listen", "unix://"])
            .current_dir(codex.workspace())
            .envs(codex.environment())
            .env("HERDR_REVIEWER_MCP_PORT", port.number().to_string())
            .env("RUST_LOG", "codex_app_server_transport=info")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut pid = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut pid)
            .unwrap();
        let (listening, ready) = mpsc::channel();
        let stderr = child.stderr.take().unwrap();
        let mut log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(codex.directory.path().join("app-server.log"))
            .unwrap();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if line.contains(LISTENING) {
                    let _ = listening.send(());
                }
                let _ = writeln!(log, "{line}");
            }
        });
        ready
            .recv_timeout(GUARD)
            .expect("the Codex app server did not listen");
        Self {
            pid: pid.trim().to_owned(),
            lifetime: Lifetime::new(child),
        }
    }

    /// Stops the server as the daemon stops to update itself, and waits until it exited.
    fn stop(mut self) {
        assert!(
            Command::new("kill")
                .args(["-TERM", &self.pid])
                .status()
                .unwrap()
                .success()
        );
        let _ = self.lifetime.child.wait();
    }
}

/// A child process that stops once its stdin closes: on drop, or when the test dies.
struct Lifetime {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Lifetime {
    fn new(mut child: Child) -> Self {
        let stdin = child.stdin.take();
        Self { child, stdin }
    }
}

impl Drop for Lifetime {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}
