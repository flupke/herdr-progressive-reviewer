//! Project-scoped registration of the reviewer's MCP endpoint in native clients.

mod document;
mod file;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use fs2::FileExt;
use review_mcp::Endpoint;

/// A native client supported by the reviewer.
#[derive(Clone, Copy, Debug)]
pub enum Client {
    Codex,
    Claude,
}

impl Client {
    pub const ALL: [Self; 2] = [Self::Codex, Self::Claude];

    pub fn config_path(self) -> &'static str {
        match self {
            Self::Codex => ".codex/config.toml",
            Self::Claude => ".mcp.json",
        }
    }
}

impl FromStr for Client {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            _ => Err("Expected codex or claude".into()),
        }
    }
}

/// Configuration shared by all reviewer instances in a project.
#[derive(Debug)]
pub struct ProjectConfig {
    root: PathBuf,
    endpoint: Endpoint,
    bridge: PathBuf,
}

impl ProjectConfig {
    pub fn new(root: &Path, endpoint: Endpoint, bridge: PathBuf) -> Self {
        Self {
            root: root.to_owned(),
            endpoint,
            bridge,
        }
    }

    pub fn endpoint(&self) -> Endpoint {
        self.endpoint
    }

    pub fn snippet(&self, client: Client) -> Result<String, String> {
        document::Configuration::new(client, self.endpoint, &self.bridge)?
            .insert("")?
            .ok_or_else(|| "Could not generate the reviewer configuration".into())
    }

    /// Register the bridge or migrate its matching HTTP entry, retaining tool policies.
    /// Returns whether configuration was written.
    pub fn install(&self, client: Client) -> Result<bool, String> {
        self.install_locked(client)
            .map_err(|error| format!("MCP setup for {}: {error}", client.config_path()))
    }

    fn install_locked(&self, client: Client) -> Result<bool, String> {
        // Lock the existing project directory: no lock file is added to the checkout.
        let lock = File::open(&self.root).map_err(|error| error.to_string())?;
        lock.lock_exclusive().map_err(|error| error.to_string())?;
        let config = file::ConfigFile::read(&self.root.join(client.config_path()))?;
        let Some(updated) = document::Configuration::new(client, self.endpoint, &self.bridge)?
            .insert(config.text())?
        else {
            return Ok(false);
        };
        config.write(&updated)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
