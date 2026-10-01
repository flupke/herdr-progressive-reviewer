use std::collections::BTreeMap;
use std::path::Path;

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
        Self {
            server,
            repository,
            context,
            port: TestPort::new(),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        self.repository.root()
    }

    pub(crate) fn open_vision(
        &self,
        recording: &Path,
        jev_script: &Path,
    ) -> anyhow::Result<Session> {
        let session = Session::new(format!(
            "vision-{}",
            self.server.root().file_name().unwrap().to_str().unwrap()
        ));
        if let Err(error) = session.run(self.run_options(recording, jev_script)) {
            let _ = session.close();
            return Err(error.into());
        }
        Ok(session)
    }

    fn run_options(&self, recording: &Path, jev_script: &Path) -> RunOptions {
        let defaults = OpenOptions::default();
        let mut environment = self.environment();
        // The vision `jev` command writes this script; the reviewer reads it
        // in place of the paid classifier.
        environment.insert(
            "HERDR_REVIEWER_JEV_SCRIPT".into(),
            jev_script.to_str().unwrap().into(),
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
                directory: Some(recording.to_owned()),
            },
        }
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
        ]);
        environment
    }
}
