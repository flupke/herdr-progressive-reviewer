use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use herdr_client::protocol::{PaneId, PluginContext, WorkspaceId};
use review_repository::repository::RepoType;
use review_test_support::{HerdrTestServer, ReviewRepositoryFixture, TestPort, repository_fixture};
use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, OpenOptions, RunOptions, Session, Size, Timeouts,
};

/// The terminal size a reviewer session starts at.
pub(crate) const SESSION_SIZE: Size = Size {
    cols: 100,
    rows: 30,
};

/// The loopback interface, which stands in for a network interface in a vision session.
#[cfg(target_os = "linux")]
const LOOPBACK: &str = "lo";
#[cfg(not(target_os = "linux"))]
const LOOPBACK: &str = "lo0";

/// The files a vision session shares with its reviewer.
pub(crate) struct VisionFiles {
    /// Where tui-test keeps the terminal recording.
    pub(crate) recording: PathBuf,
    /// The script the `jev` command writes; the reviewer reads it in place of
    /// the paid classifier.
    pub(crate) jev_script: PathBuf,
    /// Where the reviewer records the Explore prompts it sent, for the
    /// scripted agent.
    pub(crate) turns: PathBuf,
    /// Where the reviewer's stand-in browser lives. It appends each address it opens to
    /// `opened-pages` there, and fails while a file `browser-fails` exists there. A vision
    /// session never opens the browser of the desktop it runs on.
    pub(crate) browser: PathBuf,
}

pub(crate) struct ReviewWorkspace {
    server: HerdrTestServer,
    repository: Box<dyn ReviewRepositoryFixture>,
    context: PluginContext,
    port: TestPort,
}

impl ReviewWorkspace {
    pub(crate) fn exploration(repository_type: RepoType) -> Self {
        let repository = repository_fixture(repository_type);
        repository.write("sample.rs", b"pub fn before_review() {}\n");
        repository.new_change("base");
        repository.write("sample.rs", b"pub fn after_review() {}\n");
        repository.write(
            "src/math.rs",
            b"/// Add two values.\npub fn add(left: i32, right: i32) -> i32 {\n    left + right\n}\n\n/// Subtract two values.\npub fn subtract(left: i32, right: i32) -> i32 {\n    left - right\n}\n",
        );
        repository.write(
            "notes.md",
            "# Review fixture\n\nUnicode: café, 日本語, 🦀.\n\nA deliberately long line to explore wrapping and horizontal scrolling in a narrow terminal, with enough text to extend beyond the default diff pane width.\n".as_bytes(),
        );
        let server = HerdrTestServer::start(repository.root());
        let workspace = server.run_cli_json(&[
            "workspace",
            "create",
            "--cwd",
            repository.root().to_str().unwrap(),
            "--label",
            "reviewer-tui-test",
            "--no-focus",
        ]);
        let result = &workspace["result"];
        let context = PluginContext {
            workspace_id: Some(WorkspaceId(
                result["workspace"]["workspace_id"].as_str().unwrap().into(),
            )),
            focused_pane_id: Some(PaneId(
                result["root_pane"]["pane_id"].as_str().unwrap().into(),
            )),
            focused_pane_cwd: Some(repository.root().to_owned()),
            tab_id: None,
        };
        Self::start_agent(&server, &context);
        Self {
            server,
            repository,
            context,
            port: TestPort::new(),
        }
    }

    /// Make the workspace's first pane the implementation agent Explore
    /// prompts. Herdr knows agents by their process name, so a script named
    /// `claude` stands in: it reads each prompt, which the reviewer records as
    /// a turn, and shows Herdr that it works on it for two seconds, unless the
    /// file at [`Self::swallow_switch`] exists: then it reads the prompt
    /// without starting on it.
    fn start_agent(server: &HerdrTestServer, context: &PluginContext) {
        let pane = &context.focused_pane_id.as_ref().unwrap().0;
        let bin = server.root().join("agent-bin");
        fs::create_dir_all(&bin).unwrap();
        let agent = bin.join("claude");
        let switch = Self::swallow_switch_in(server);
        fs::write(
            &agent,
            format!(
                "#!/bin/sh\nstty -echo 2>/dev/null\nidle=\nwhile IFS= read -r line; do\n  \
                 [ -e '{}' ] && continue\n  \
                 printf '\\033]0;\\342\\240\\213 Working\\007'\n  \
                 [ -n \"$idle\" ] && kill \"$idle\" 2>/dev/null\n  \
                 (sleep 2; printf '\\033]0;\\342\\234\\263 Ready\\007') &\n  \
                 idle=$!\ndone\n",
                switch.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&agent, fs::Permissions::from_mode(0o755)).unwrap();
        let command = format!("exec '{}'", agent.display());
        server.run_cli(&["pane", "run", pane, &command]);
    }

    /// The file whose presence makes the stand-in agent swallow each prompt
    /// without starting on it.
    pub(crate) fn swallow_switch(&self) -> PathBuf {
        Self::swallow_switch_in(&self.server)
    }

    fn swallow_switch_in(server: &HerdrTestServer) -> PathBuf {
        server.root().join("agent-swallows-prompts")
    }

    pub(crate) fn mcp_port(&self) -> u16 {
        self.port.number()
    }

    pub(crate) fn root(&self) -> &Path {
        self.repository.root()
    }

    pub(crate) fn open_vision(&self, files: &VisionFiles) -> anyhow::Result<Session> {
        let session = Session::new(format!(
            "vision-{}",
            self.server.root().file_name().unwrap().to_str().unwrap()
        ));
        if let Err(error) = session.run(self.run_options(files)) {
            let _ = session.close();
            return Err(error.into());
        }
        Ok(session)
    }

    /// Run the Herdr action that opens the Explore page, as Herdr runs it in this
    /// workspace, with a browser that writes the address it opens under
    /// `directory`. Returns that address, with its token.
    pub(crate) fn open_explore_page(&self, directory: &Path) -> anyhow::Result<String> {
        let reviewer = PathBuf::from(std::env::var("REVIEWER_BIN_PATH")?);
        let opened = directory.join("explore-page-url");
        let _ = fs::remove_file(&opened);
        let browser = directory.join("browser");
        fs::write(
            &browser,
            format!("#!/bin/sh\nprintf %s \"$1\" > '{}'\n", opened.display()),
        )?;
        fs::set_permissions(&browser, fs::Permissions::from_mode(0o755))?;
        let output = std::process::Command::new(reviewer.with_file_name("reviewer-control"))
            .arg("explore-page")
            .env_clear()
            .envs(self.environment())
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("BROWSER", &browser)
            .current_dir(self.repository.root())
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "the action failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(fs::read_to_string(&opened)?)
    }

    fn run_options(&self, files: &VisionFiles) -> RunOptions {
        let defaults = OpenOptions::default();
        let mut environment = self.environment();
        environment.insert(
            "BROWSER".into(),
            Self::stand_in_browser(&files.browser)
                .to_str()
                .unwrap()
                .into(),
        );
        environment.insert(
            "HERDR_REVIEWER_JEV_SCRIPT".into(),
            files.jev_script.to_str().unwrap().into(),
        );
        environment.insert(
            "HERDR_REVIEWER_VISION_TURNS".into(),
            files.turns.to_str().unwrap().into(),
        );
        // tui-test adds environment overrides to its inherited environment.
        // env -u removes live Herdr context without putting environment values
        // (which may contain credentials) in process arguments or traces.
        let mut args = vec![
            "-u".into(),
            "TYPESAFE_API_KEY".into(),
            "-u".into(),
            "NO_COLOR".into(),
        ];
        for (name, _) in std::env::vars_os() {
            let name = name.to_str().unwrap();
            if name.starts_with("HERDR_") && !environment.contains_key(name) {
                args.extend(["-u".into(), name.into()]);
            }
        }
        args.push(std::env::var("REVIEWER_BIN_PATH").expect("run with make vision"));
        RunOptions {
            program: "/usr/bin/env".into(),
            args,
            cwd: Some(self.repository.root().to_str().unwrap().into()),
            env: environment.into_iter().collect(),
            cols: SESSION_SIZE.cols,
            rows: SESSION_SIZE.rows,
            wait_ready: Some(false),
            backend: defaults.backend,
            profile: defaults.profile,
            restart: false,
            timeouts: Timeouts {
                text: Some(10_000),
                exit: Some(10_000),
                ..Timeouts::default()
            },
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::Always,
                directory: Some(files.recording.clone()),
            },
        }
    }

    /// Writes the stand-in browser of [`VisionFiles::browser`] under `directory`.
    fn stand_in_browser(directory: &Path) -> PathBuf {
        let browser = directory.join("stand-in-browser");
        fs::write(
            &browser,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{opened}'\n[ ! -e '{fails}' ] || exit 3\n",
                opened = directory.join("opened-pages").display(),
                fails = directory.join("browser-fails").display(),
            ),
        )
        .unwrap();
        fs::set_permissions(&browser, fs::Permissions::from_mode(0o755)).unwrap();
        browser
    }

    fn environment(&self) -> BTreeMap<String, String> {
        let mut environment: BTreeMap<_, _> = self
            .server
            .environment()
            .iter()
            .filter(|(name, _)| {
                let name = name.to_str().unwrap();
                name.starts_with("HERDR_") || name.starts_with("XDG_") || name == "SHELL"
            })
            .map(|(name, value)| {
                (
                    name.to_str().unwrap().into(),
                    value.to_str().unwrap().into(),
                )
            })
            .collect();
        environment.extend([
            ("HERDR_REVIEWER_VISION".into(), "1".into()),
            (
                "HERDR_PLUGIN_ID".into(),
                "herdr.progressive-reviewer".into(),
            ),
            (
                "HERDR_PLUGIN_STATE_DIR".into(),
                self.server.state_directory().to_str().unwrap().into(),
            ),
            (
                "HERDR_WORKSPACE_ID".into(),
                self.context.workspace_id.as_ref().unwrap().0.clone(),
            ),
            (
                "HERDR_PLUGIN_CONTEXT_JSON".into(),
                serde_json::to_string(&self.context).unwrap(),
            ),
            (
                "HERDR_REVIEWER_MCP_PORT".into(),
                self.port.number().to_string(),
            ),
            // The Explore page's network listener, and the pane's QR code, on the loopback
            // interface: shown as on a real network, reachable from this machine only.
            ("HERDR_REVIEWER_EXPLORE_NETWORK".into(), "on".into()),
            ("HERDR_REVIEWER_EXPLORE_INTERFACE".into(), LOOPBACK.into()),
            ("HERDR_REVIEWER_EXPLORE_PORT".into(), "0".into()),
        ]);
        environment
    }
}
