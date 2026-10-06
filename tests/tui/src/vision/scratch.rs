//! The scratch repository a vision session reviews, and the files the session shares with its
//! reviewer: its private state, its stand-in agent, browser and Jev script, and the turns it
//! records.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use herdr_client::protocol::{PluginContext, WorkspaceId};
use review_explore_page_settings::ExplorePageSettings;
use review_repository::repository::RepoType;
use review_test_support::{
    ReviewRepositoryFixture, TestPort, repository_fixture, write_reviewer_settings,
};

/// The loopback interface, which stands in for a network interface in a vision session.
#[cfg(target_os = "linux")]
const LOOPBACK: &str = "lo";
#[cfg(not(target_os = "linux"))]
const LOOPBACK: &str = "lo0";

/// The stand-in agent's script; `SWALLOW_SWITCH` stands for the path of the switch.
const STAND_IN_AGENT: &str = r#"#!/bin/sh
stty -echo 2>/dev/null
report() {
  printf '\033]0;%s\007' "$2"
  "${HERDR_BIN_PATH:-herdr}" pane report-agent "$HERDR_PANE_ID" --source reviewer-vision \
    --agent claude --state "$1" >/dev/null 2>&1
}
report idle '✳ Ready'
idle=
while IFS= read -r line; do
  [ -e 'SWALLOW_SWITCH' ] && continue
  report working '⠋ Working'
  [ -n "$idle" ] && kill "$idle" 2>/dev/null
  (sleep 2; report idle '✳ Ready') &
  idle=$!
done
"#;

/// The files of a scratch repository: the base change, then the change under review.
#[derive(Clone, Debug, Default)]
pub(crate) struct Seed {
    pub(crate) base: BTreeMap<String, String>,
    pub(crate) change: BTreeMap<String, String>,
}

impl Seed {
    /// The default change: a rename in one file, two new files, Unicode and a long line.
    pub(crate) fn sample() -> Self {
        let file = |path: &str, content: &str| (path.to_owned(), content.to_owned());
        Self {
            base: BTreeMap::from([file("sample.rs", "pub fn before_review() {}\n")]),
            change: BTreeMap::from([
                file("sample.rs", "pub fn after_review() {}\n"),
                file(
                    "src/math.rs",
                    "/// Add two values.\npub fn add(left: i32, right: i32) -> i32 {\n    left + right\n}\n\n/// Subtract two values.\npub fn subtract(left: i32, right: i32) -> i32 {\n    left - right\n}\n",
                ),
                file(
                    "notes.md",
                    "# Review fixture\n\nUnicode: café, 日本語, 🦀.\n\nA deliberately long line to explore wrapping and horizontal scrolling in a narrow terminal, with enough text to extend beyond the default diff pane width.\n",
                ),
            ]),
        }
    }
}

/// A scratch repository and the session's files, in `directory`, which outlives the session.
pub(crate) struct Scratch {
    repository: Box<dyn ReviewRepositoryFixture>,
    directory: PathBuf,
    port: TestPort,
}

impl Scratch {
    pub(crate) fn create(kind: RepoType, seed: &Seed, directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(
            directory
                .parent()
                .context("the session directory needs a parent")?,
        )?;
        fs::create_dir(&directory).context("the session directory must be new")?;
        let repository = repository_fixture(kind);
        for (path, content) in &seed.base {
            repository.write(path, content.as_bytes());
        }
        repository.new_change("base");
        for (path, content) in &seed.change {
            repository.write(path, content.as_bytes());
        }
        let scratch = Self {
            repository,
            directory,
            port: TestPort::new(),
        };
        fs::create_dir_all(scratch.state())?;
        fs::create_dir_all(scratch.turns())?;
        // The Explore page's network listener, and the pane's QR code, on the loopback
        // interface: shown as on a real network, reachable from this machine only, on any free
        // port.
        let mut settings = ExplorePageSettings::default();
        settings.network.set_interface(LOOPBACK);
        settings.network.first_port = 0;
        write_reviewer_settings(&scratch.state(), &settings);
        scratch.write_agent()?;
        scratch.write_browser()?;
        Ok(scratch)
    }

    pub(crate) fn root(&self) -> &Path {
        self.repository.root()
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn mcp_port(&self) -> u16 {
        self.port.number()
    }

    /// The reviewer's private state directory.
    fn state(&self) -> PathBuf {
        self.directory.join("state")
    }

    /// The script the `jev` tool writes; the reviewer reads it in place of the paid classifier.
    pub(crate) fn jev_script(&self) -> PathBuf {
        self.directory.join("jev-script.json")
    }

    /// Where the reviewer records the Explore prompts it sent, for the scripted agent.
    pub(crate) fn turns(&self) -> PathBuf {
        self.directory.join("turns")
    }

    /// The file whose presence makes the stand-in agent read each prompt without starting on it.
    pub(crate) fn swallow_switch(&self) -> PathBuf {
        self.directory.join("agent-swallows-prompts")
    }

    /// The program of the stand-in agent's pane.
    pub(crate) fn agent_command(&self) -> Vec<String> {
        vec![path_string(&self.directory.join("agent-bin/claude"))]
    }

    /// The program of the reviewer's pane: `reviewer`, without the paid classifier's key, in
    /// colors whatever the user's terminal says.
    pub(crate) fn reviewer_command(reviewer: &Path) -> Vec<String> {
        ["/usr/bin/env", "-u", "TYPESAFE_API_KEY", "-u", "NO_COLOR"]
            .into_iter()
            .map(str::to_owned)
            .chain([path_string(reviewer)])
            .collect()
    }

    /// The environment the reviewer runs with in Herdr workspace `workspace`, as a Herdr plugin
    /// with private state, in a vision session.
    pub(crate) fn reviewer_environment(
        &self,
        workspace: &str,
        socket: &Path,
    ) -> Result<BTreeMap<String, String>> {
        let context = PluginContext {
            workspace_id: Some(WorkspaceId(workspace.to_owned())),
            tab_id: None,
            focused_pane_id: None,
            focused_pane_cwd: Some(self.root().to_owned()),
        };
        let mut environment = BTreeMap::from([
            ("HERDR_REVIEWER_VISION", "1".to_owned()),
            ("HERDR_PLUGIN_ID", "herdr.progressive-reviewer".to_owned()),
            ("HERDR_PLUGIN_STATE_DIR", path_string(&self.state())),
            ("HERDR_WORKSPACE_ID", workspace.to_owned()),
            (
                "HERDR_PLUGIN_CONTEXT_JSON",
                serde_json::to_string(&context)?,
            ),
            ("HERDR_REVIEWER_MCP_PORT", self.mcp_port().to_string()),
            ("HERDR_SOCKET_PATH", path_string(socket)),
            ("HERDR_REVIEWER_JEV_SCRIPT", path_string(&self.jev_script())),
            ("HERDR_REVIEWER_VISION_TURNS", path_string(&self.turns())),
            (
                "BROWSER",
                path_string(&self.directory.join("stand-in-browser")),
            ),
        ])
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<BTreeMap<_, _>>();
        if let Some(path) = std::env::var_os("PATH") {
            environment.insert("PATH".into(), path.to_string_lossy().into_owned());
        }
        Ok(environment)
    }

    /// Run the Herdr action that opens the Explore page, as Herdr runs it in the session's
    /// workspace, with a browser that records the address it opens. Returns that address, with
    /// its token.
    pub(crate) fn open_explore_page(
        &self,
        reviewer: &Path,
        environment: &BTreeMap<String, String>,
    ) -> Result<String> {
        let opened = self.directory.join("explore-page-url");
        let _ = fs::remove_file(&opened);
        let browser = self.directory.join("browser");
        write_script(
            &browser,
            &format!("#!/bin/sh\nprintf %s \"$1\" > '{}'\n", opened.display()),
        )?;
        let output = std::process::Command::new(reviewer.with_file_name("reviewer-control"))
            .arg("explore-page")
            .envs(environment)
            .env("BROWSER", &browser)
            .current_dir(self.root())
            .output()?;
        ensure!(
            output.status.success(),
            "the action failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(fs::read_to_string(&opened)?)
    }

    /// The stand-in implementation agent that Explore prompts. Herdr knows agents by their
    /// process name, so a script named `claude` stands in: it reads each prompt, which the
    /// reviewer records as a turn, and tells Herdr that it works on it for two seconds, unless
    /// the file at [`Self::swallow_switch`] exists: then it reads the prompt without starting on
    /// it. It reports its state to Herdr itself, as Claude's hooks do, since the user's Herdr may
    /// not read it from the screen; its titles are those of Claude Code.
    fn write_agent(&self) -> Result<()> {
        let bin = self.directory.join("agent-bin");
        fs::create_dir_all(&bin)?;
        write_script(
            &bin.join("claude"),
            &STAND_IN_AGENT.replace("SWALLOW_SWITCH", &path_string(&self.swallow_switch())),
        )
    }

    /// The reviewer's browser: it appends each address it opens to `opened-pages` in the
    /// session directory, and fails with exit status 3 while a file `browser-fails` exists
    /// there. A vision session never opens the user's browser.
    fn write_browser(&self) -> Result<()> {
        write_script(
            &self.directory.join("stand-in-browser"),
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{opened}'\n[ ! -e '{fails}' ] || exit 3\n",
                opened = self.directory.join("opened-pages").display(),
                fails = self.directory.join("browser-fails").display(),
            ),
        )
    }
}

fn write_script(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
