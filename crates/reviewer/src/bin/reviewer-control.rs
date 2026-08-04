//! Progressive reviewer action process.

use std::env;
use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use fs2::FileExt;
use herdr_client::client::HerdrClient;
use herdr_client::protocol::PluginContext;
use reviewer::control::{PaneAction, PaneActions};

#[derive(Debug)]
struct Control {
    action: PaneAction,
    context: PluginContext,
    client: HerdrClient,
    _lock: File,
}

impl Control {
    fn from_env() -> eyre::Result<Self> {
        let action = match env::args().nth(1).as_deref() {
            Some("open") => PaneAction::Open,
            Some("close") => PaneAction::Close,
            Some("toggle") => PaneAction::Toggle,
            _ => eyre::bail!("usage: reviewer-control <open|close|toggle>"),
        };
        let context = serde_json::from_str(
            &env::var("HERDR_PLUGIN_CONTEXT_JSON")
                .map_err(|_| eyre::eyre!("HERDR_PLUGIN_CONTEXT_JSON is not set"))?,
        )?;
        let state_dir = PathBuf::from(
            env::var_os("HERDR_PLUGIN_STATE_DIR")
                .ok_or_else(|| eyre::eyre!("HERDR_PLUGIN_STATE_DIR is not set"))?,
        );
        std::fs::create_dir_all(&state_dir)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(state_dir.join("actions.lock"))?;
        lock.lock_exclusive()?;
        Ok(Self {
            action,
            context,
            client: HerdrClient::from_env()?,
            _lock: lock,
        })
    }

    fn run(&self) -> eyre::Result<()> {
        PaneActions::new(&self.client).run(self.action, &self.context)?;
        Ok(())
    }
}

fn main() -> eyre::Result<()> {
    Control::from_env()?.run()
}
