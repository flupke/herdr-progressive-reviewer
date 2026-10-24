//! A real Codex TUI in a Herdr test server, on an unmanaged app server of its own with a
//! scratch Codex home and a local model fixture (`codex_provider.py`): it calls no model API,
//! and never touches the user's `~/.codex`. `.agents/wiki/codex-daemon.md` says what it works
//! around.

use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;

use herdr_client::protocol::{AgentStatus, HerdrEvent, PaneId};

use crate::{GUARD, HerdrTestServer};

/// Submits `text` to the agent of `pane`, as the reviewer and the user do.
pub fn prompt(herdr: &HerdrTestServer, pane: &PaneId, text: &str) {
    herdr.run_cli(&["agent", "prompt", &pane.0, text]);
}

/// Waits until the pane `pane` of `herdr` shows `text`, as Herdr reads it.
pub fn wait_output(herdr: &HerdrTestServer, pane: &PaneId, text: &str) {
    let guard = GUARD.as_millis().to_string();
    herdr.run_cli(&[
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

/// A scratch Codex home, with its user's home and the workspace the agent works in.
pub struct CodexHome {
    directory: tempfile::TempDir,
    codex: PathBuf,
}

impl Default for CodexHome {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexHome {
    pub fn new() -> Self {
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

    /// The scratch directory that holds the homes, and the fixtures' logs.
    fn root(&self) -> &Path {
        self.directory.path()
    }

    pub fn workspace(&self) -> PathBuf {
        self.root().join("workspace")
    }

    /// The Codex home, `CODEX_HOME`.
    pub fn home(&self) -> PathBuf {
        self.root().join("codex")
    }

    /// The variables every Codex process of this home runs with.
    pub fn environment(&self) -> [(&'static str, PathBuf); 2] {
        [
            ("CODEX_HOME", self.home()),
            ("HOME", self.root().join("home")),
        ]
    }

    /// Writes the configuration that has Codex use the model fixture listening on `provider`,
    /// and trust the workspace, followed by `mcp`, the TOML that registers the reviewer's MCP
    /// server `herdr_reviewer`, whose `list_threads` it may call without asking.
    pub fn configure(&self, provider: u16, mcp: &str) {
        let workspace = serde_json::to_string(&self.workspace()).unwrap();
        fs::write(
            self.home().join("config.toml"),
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

{mcp}
[mcp_servers.herdr_reviewer.tools.list_threads]
approval_mode = "approve"
"#
            ),
        )
        .unwrap();
    }

    /// Starts the Codex TUI with `arguments` in a pane of `herdr`, and returns once Herdr
    /// reports it idle: it takes what is typed from then on.
    pub fn start_tui(&self, herdr: &HerdrTestServer, arguments: &[&str]) -> PaneId {
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
/// Node wrapper: a test signals the app server itself.
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

/// The local model fixture: a prompt that says "call the reviewer" has the agent call the
/// reviewer's `list_threads`; every other turn ends with the message `Answered: <prompt>`.
pub struct Provider {
    port: u16,
    _child: Lifetime,
}

impl Provider {
    /// Starts the fixture, and configures `codex` to use it, with `mcp` as
    /// [`CodexHome::configure`] takes it.
    pub fn start(codex: &CodexHome, mcp: &str) -> Self {
        let mut child = Command::new("python3")
            .arg("-B")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/codex_provider.py"
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(fs::File::create(codex.root().join("provider.log")).unwrap())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = line.trim().parse().expect("the provider printed no port");
        codex.configure(port, mcp);
        Self {
            port,
            _child: Lifetime::new(child),
        }
    }
}

/// An unmanaged Codex app server on the home's control socket, which the TUI takes for its
/// daemon: unlike the managed daemon, it never updates itself. It runs outside the test's
/// Herdr, without the `HERDR_*` variables of the test's own environment.
pub struct AppServer {
    pid: String,
    lifetime: Lifetime,
}

/// What the app server writes once its control socket accepts connections.
const LISTENING: &str = "app-server control socket listening";

impl AppServer {
    /// Starts the server of `codex`, with the variables `environment` too, and returns once it
    /// listens.
    pub fn start(codex: &CodexHome, provider: &Provider, environment: &[(&str, &OsStr)]) -> Self {
        assert_ne!(provider.port, 0);
        // The shell stops the server once the test lets go of its stdin, even by dying.
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                r#"exec 3<&0; "$@" </dev/null >/dev/null & server=$!; echo "$server"
                   { cat <&3 >/dev/null; kill -TERM "$server" 2>/dev/null; } & wait "$server""#,
                "sh",
            ])
            .arg(&codex.codex)
            .args(["app-server", "--listen", "unix://"])
            .current_dir(codex.workspace());
        for (name, _) in std::env::vars_os() {
            if name.as_encoded_bytes().starts_with(b"HERDR_") {
                command.env_remove(name);
            }
        }
        let mut child = command
            .envs(codex.environment())
            .envs(environment.iter().copied())
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
            .open(codex.root().join("app-server.log"))
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
    pub fn stop(mut self) {
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
