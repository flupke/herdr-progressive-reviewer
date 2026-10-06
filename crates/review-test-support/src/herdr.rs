use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;

use herdr_client::client::HerdrClient;
use herdr_client::protocol::PaneId;

use review_explore_page_settings::ExplorePageSettings;

use crate::detection_rules::DetectionRules;

/// Writes the reviewer settings file of the state directory `state` with the Explore page
/// `settings`, as the reviewer saves them, and the defaults of the other settings.
pub fn write_reviewer_settings(state: &Path, settings: &ExplorePageSettings) {
    let saved = serde_json::json!({ "explore_page": settings });
    fs::write(
        state.join("settings.json"),
        serde_json::to_vec(&saved).unwrap(),
    )
    .unwrap();
}

/// What Herdr prints once its API socket accepts requests.
const READY_LINE: &str = "herdr server running";

/// A real Herdr server with private sockets, configuration, and state.
///
/// Its private directory goes once the server stops: when it drops, and also when the test
/// process dies without dropping it.
pub struct HerdrTestServer {
    directory: tempfile::TempDir,
    binary: PathBuf,
    repository_root: PathBuf,
    socket_path: PathBuf,
    state_directory: PathBuf,
    environment: BTreeMap<OsString, OsString>,
    child: Option<Child>,
}

impl HerdrTestServer {
    /// Start an isolated server, and return once its API socket accepts requests.
    pub fn start(repository_root: &Path) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let config_directory = config_directory(directory.path());
        let herdr_config_directory = herdr_config_directory(directory.path());
        let runtime_directory = directory.path().join("runtime");
        let state_directory = directory.path().join("state");
        let config_path = herdr_config_directory.join("config.toml");
        let socket_path = directory.path().join("herdr.sock");
        fs::create_dir_all(&herdr_config_directory).unwrap();
        fs::create_dir_all(&runtime_directory).unwrap();
        fs::create_dir_all(&state_directory).unwrap();
        // Detect agents with the repository's rules, and never download others.
        fs::write(
            &config_path,
            "onboarding = false\n[update]\nversion_check = false\nmanifest_check = false\n",
        )
        .unwrap();
        for rules in &DetectionRules::ALL {
            rules.install(&herdr_config_directory);
        }
        let mut environment: BTreeMap<_, _> = std::env::vars_os()
            .filter(|(name, _)| !name.as_encoded_bytes().starts_with(b"HERDR_"))
            .collect();
        environment.extend([
            (
                "HERDR_SOCKET_PATH".into(),
                socket_path.clone().into_os_string(),
            ),
            ("HERDR_CONFIG_PATH".into(), config_path.into_os_string()),
            ("XDG_CONFIG_HOME".into(), config_directory.into_os_string()),
            ("XDG_RUNTIME_DIR".into(), runtime_directory.into_os_string()),
            (
                "XDG_STATE_HOME".into(),
                state_directory.clone().into_os_string(),
            ),
            ("SHELL".into(), "/bin/sh".into()),
        ]);
        // A test reviewer serves its Explore page on this machine only.
        let mut settings = ExplorePageSettings::default();
        settings.network.enabled = false;
        write_reviewer_settings(&state_directory, &settings);
        let mut server = Self {
            directory,
            // The dev shell pins this release. HERDR_BIN_PATH is not used:
            // Herdr sets it to its own binary in every pane.
            binary: std::env::var_os("TEST_HERDR_BIN_PATH")
                .map_or_else(|| PathBuf::from("herdr"), PathBuf::from),
            repository_root: repository_root.to_owned(),
            socket_path,
            state_directory,
            environment,
            child: None,
        };
        let output = File::create(server.root().join("server.log")).unwrap();
        let mut child = server
            .lifetime_command()
            .stdin(Stdio::piped())
            .stdout(output.try_clone().unwrap())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("could not start {}: {error}", server.binary.display()));
        let ready = copy_until_ready(child.stderr.take().unwrap(), output);
        server.child = Some(child);
        if ready.recv_timeout(crate::GUARD).is_err() {
            let log = fs::read_to_string(server.root().join("server.log")).unwrap_or_default();
            panic!("isolated Herdr server did not become ready:\n{log}");
        }
        server
    }

    /// Return the private fixture directory.
    pub fn root(&self) -> &Path {
        self.directory.path()
    }

    /// Return the configured Herdr executable.
    pub fn binary(&self) -> &Path {
        &self.binary
    }

    /// Return the private state directory.
    pub fn state_directory(&self) -> &Path {
        &self.state_directory
    }

    /// Give the reviewers this server starts the Explore page `settings`, in place of the
    /// default of the tests, which keeps the page off the network.
    pub fn set_explore_page_settings(&self, settings: &ExplorePageSettings) {
        write_reviewer_settings(&self.state_directory, settings);
    }

    /// Return the environment for processes connected to this server.
    pub fn environment(&self) -> &BTreeMap<OsString, OsString> {
        &self.environment
    }

    /// Follow the events the reviewer follows: focus and agent detection.
    pub fn events(&self) -> crate::HerdrEventWatch {
        crate::HerdrEventWatch::start(&self.client())
    }

    /// Follow the status of the agent of `pane`, which this reports first.
    pub fn agent_statuses(&self, pane: &PaneId) -> crate::AgentStatusWatch {
        crate::AgentStatusWatch::start(&self.client(), pane)
    }

    /// Connect a reviewer client to this server.
    pub fn client(&self) -> HerdrClient {
        HerdrClient::new(
            self.socket_path.clone(),
            "herdr.progressive-reviewer".to_owned(),
            self.state_directory.clone(),
        )
    }

    /// Run a CLI command against this server and require success.
    pub fn run_cli(&self, arguments: &[&str]) -> Output {
        let output = self.command().args(arguments).output().unwrap();
        assert!(
            output.status.success(),
            "herdr {} failed:\n{}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    /// Run a CLI command and decode its JSON response.
    pub fn run_cli_json(&self, arguments: &[&str]) -> serde_json::Value {
        serde_json::from_slice(&self.run_cli(arguments).stdout).unwrap()
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .current_dir(&self.repository_root)
            .env_clear()
            .envs(&self.environment);
        command
    }

    /// Run the server under a shell that stops it once this process lets go
    /// of the shell's stdin: on drop, and also when this process dies without
    /// dropping anything, since the system then closes the pipe. A server
    /// that ignores SIGTERM gets SIGKILL five seconds later, and one that
    /// stops by itself takes its watcher with it. Once the server stopped
    /// because this process let go, the shell removes the server's private
    /// directory, which a killed test would leave behind.
    fn lifetime_command(&self) -> Command {
        // A background job reads /dev/null unless told otherwise, so the
        // watcher reads the pipe through a saved descriptor. The watcher
        // writes nothing to stderr, which this process reads until the
        // server and the shell close it.
        const SCRIPT: &str = r#"exec 3<&0
"$0" server </dev/null &
server=$!
{
  cat <&3 >/dev/null
  : >"$1/.released"
  kill "$server" 2>/dev/null
  sleep 5
  kill -9 "$server" 2>/dev/null
} 2>/dev/null &
watcher=$!
wait "$server"
status=$?
kill "$watcher" 2>/dev/null
if [ -n "$1" ] && [ -e "$1/.released" ]; then
  rm -rf "$1"
fi
exit "$status""#;
        let mut command = Command::new("sh");
        command
            .args(["-c", SCRIPT])
            .arg(&self.binary)
            .arg(self.root())
            .current_dir(&self.repository_root)
            .env_clear()
            .envs(&self.environment);
        command
    }
}

/// Copies what the server writes to its stderr to `log`, on another thread, and says on the
/// channel it returns once the server's API socket accepts requests. The channel disconnects
/// without a word when the server stops first.
fn copy_until_ready(stderr: ChildStderr, mut log: File) -> mpsc::Receiver<()> {
    let (ready, said) = mpsc::channel();
    thread::spawn(move || {
        let mut ready = Some(ready);
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else {
                return;
            };
            let _ = writeln!(log, "{line}");
            if line.contains(READY_LINE)
                && let Some(ready) = ready.take()
            {
                let _ = ready.send(());
            }
        }
    });
    said
}

/// The configuration directory (`XDG_CONFIG_HOME`) of the server whose files
/// are in `root`.
fn config_directory(root: &Path) -> PathBuf {
    root.join("config")
}

/// Herdr's own configuration directory, inside [`config_directory`].
fn herdr_config_directory(root: &Path) -> PathBuf {
    config_directory(root).join("herdr")
}

impl Drop for HerdrTestServer {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // Closing the shell's stdin stops the server, at the latest with SIGKILL five seconds
        // later; the shell then exits.
        drop(child.stdin.take());
        let _ = child.wait();
    }
}

#[cfg(test)]
#[path = "herdr.tests.rs"]
mod tests;
