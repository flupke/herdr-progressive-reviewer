//! `reviewer-control explore-page`, the Herdr action that opens the Explore page of the
//! workspace's open reviewer in the default browser.
use std::env;
use std::path::PathBuf;

use herdr_client::protocol::PluginContext;
use review_explore_page_host::{Browser, PageDirectory, PageOpener};

/// The action, for the workspace it runs in.
pub(super) struct OpenExplorePage(PageOpener);

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
        let workspace = context
            .workspace_id
            .ok_or_else(|| eyre::eyre!("Herdr gave the action no workspace"))?;
        Ok(Self(PageOpener::new(
            PageDirectory::new(&state_dir),
            workspace,
            Browser::from_env(),
        )))
    }

    pub(super) fn run(&self) -> eyre::Result<()> {
        Ok(self.0.open()?)
    }
}
