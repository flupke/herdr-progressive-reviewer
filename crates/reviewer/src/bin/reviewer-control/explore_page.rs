//! `reviewer-control explore-page`, the Herdr action that opens the Explore page of the
//! workspace's open reviewer in the default browser.
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use herdr_client::protocol::{PluginContext, WorkspaceId};
use review_explore_page_host::PageDirectory;

/// The action, for the workspace it runs in.
pub(super) struct OpenExplorePage {
    workspace: WorkspaceId,
    pages: PageDirectory,
    browser: Browser,
}

impl OpenExplorePage {
    /// The action in the workspace of Herdr's action context.
    pub(super) fn from_env() -> eyre::Result<Self> {
        let context: PluginContext = serde_json::from_str(
            &env::var("HERDR_PLUGIN_CONTEXT_JSON")
                .map_err(|_| eyre::eyre!("HERDR_PLUGIN_CONTEXT_JSON is not set"))?,
        )?;
        let state_dir = PathBuf::from(
            env::var_os("HERDR_PLUGIN_STATE_DIR")
                .ok_or_else(|| eyre::eyre!("HERDR_PLUGIN_STATE_DIR is not set"))?,
        );
        Ok(Self {
            workspace: context
                .workspace_id
                .ok_or_else(|| eyre::eyre!("Herdr gave the action no workspace"))?,
            pages: PageDirectory::new(&state_dir),
            browser: Browser::from_env(),
        })
    }

    pub(super) fn run(&self) -> eyre::Result<()> {
        let url = self.pages.address(&self.workspace)?.ok_or_else(|| {
            eyre::eyre!(
                "No open reviewer in this workspace: open the progressive reviewer, then its Explore page"
            )
        })?;
        self.browser.open(&url)
    }
}

/// The program that opens an address in the default browser: `$BROWSER` when it is set (a
/// program and its arguments, separated by spaces), `xdg-open` on Linux, `open` on macOS.
struct Browser {
    program: OsString,
    arguments: Vec<OsString>,
}

impl Browser {
    fn from_env() -> Self {
        match env::var("BROWSER") {
            Ok(command) if !command.trim().is_empty() => Self::command(&command),
            _ if cfg!(target_os = "macos") => Self::command("open"),
            _ => Self::command("xdg-open"),
        }
    }

    fn command(command: &str) -> Self {
        let mut words = command.split_whitespace().map(OsString::from);
        Self {
            program: words.next().unwrap_or_default(),
            arguments: words.collect(),
        }
    }

    fn open(&self, url: &str) -> eyre::Result<()> {
        let status = Command::new(&self.program)
            .args(&self.arguments)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .map_err(|error| {
                eyre::eyre!(
                    "Cannot open the Explore page with {}: {error}",
                    self.program.to_string_lossy()
                )
            })?;
        eyre::ensure!(
            status.success(),
            "{} could not open the Explore page ({status})",
            self.program.to_string_lossy()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use review_explore_page::RoundPublisher;
    use review_explore_page_host::PageHost;

    use super::*;

    /// A browser that writes the address it opens to `opened`.
    fn recording_browser(directory: &Path, opened: &Path) -> Browser {
        let script = directory.join("browser");
        std::fs::write(
            &script,
            format!("#!/bin/sh\nprintf %s \"$2\" > '{}'\n", opened.display()),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        Browser::command(&format!("{} --new-tab", script.display()))
    }

    #[test]
    fn the_action_opens_the_page_of_the_workspaces_reviewer_in_the_browser() {
        let state = tempfile::tempdir().unwrap();
        let workspace = WorkspaceId("w1".into());
        let round = RoundPublisher::default();
        let host = PageHost::start(
            round.subscribe(),
            &PageDirectory::new(state.path()),
            &workspace,
        )
        .unwrap();
        let opened = state.path().join("opened");
        let action = OpenExplorePage {
            workspace,
            pages: PageDirectory::new(state.path()),
            browser: recording_browser(state.path(), &opened),
        };

        action.run().unwrap();

        assert_eq!(std::fs::read_to_string(&opened).unwrap(), host.url());
    }

    #[test]
    fn without_an_open_reviewer_the_action_opens_nothing_and_says_why() {
        let state = tempfile::tempdir().unwrap();
        let opened = state.path().join("opened");
        let action = OpenExplorePage {
            workspace: WorkspaceId("w1".into()),
            pages: PageDirectory::new(state.path()),
            browser: recording_browser(state.path(), &opened),
        };

        assert!(action.run().is_err());
        assert!(!opened.exists());
    }
}
