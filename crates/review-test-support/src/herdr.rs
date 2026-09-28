use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use herdr_client::client::HerdrClient;

/// A real Herdr server with private sockets, configuration, and state.
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
    /// Start an isolated server and wait for its API socket.
    pub fn start(repository_root: &Path) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let config_directory = directory.path().join("config");
        let runtime_directory = directory.path().join("runtime");
        let state_directory = directory.path().join("state");
        let config_path = config_directory.join("herdr/config.toml");
        let socket_path = directory.path().join("herdr.sock");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::create_dir_all(&runtime_directory).unwrap();
        fs::create_dir_all(&state_directory).unwrap();
        // Use the installed binary's rules without background network updates.
        fs::write(
            &config_path,
            "onboarding = false\n[update]\nversion_check = false\nmanifest_check = false\n",
        )
        .unwrap();
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
        let mut server = Self {
            directory,
            binary: std::env::var_os("HERDR_BIN_PATH")
                .map_or_else(|| PathBuf::from("herdr"), PathBuf::from),
            repository_root: repository_root.to_owned(),
            socket_path,
            state_directory,
            environment,
            child: None,
        };
        let output = File::create(server.root().join("server.log")).unwrap();
        server.child = Some(
            server
                .command()
                .arg("server")
                .stdin(Stdio::null())
                .stdout(output.try_clone().unwrap())
                .stderr(output)
                .spawn()
                .unwrap_or_else(|error| {
                    panic!("could not start {}: {error}", server.binary.display())
                }),
        );
        server.wait_until_ready();
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

    /// Return the environment for processes connected to this server.
    pub fn environment(&self) -> &BTreeMap<OsString, OsString> {
        &self.environment
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

    fn wait_until_ready(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if UnixStream::connect(&self.socket_path).is_ok() {
                return;
            }
            if self.child.as_mut().unwrap().try_wait().unwrap().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        let log = fs::read_to_string(self.root().join("server.log")).unwrap_or_default();
        panic!("isolated Herdr server did not become ready:\n{log}");
    }
}

impl Drop for HerdrTestServer {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
