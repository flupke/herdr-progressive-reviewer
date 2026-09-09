use std::path::{Path, PathBuf};

use super::{Client, document::Configuration, file::ConfigFile};

/// Register the bridge once for every repository a native client opens.
pub struct UserConfig {
    client: Client,
    directory: PathBuf,
    bridge: PathBuf,
}

impl UserConfig {
    pub fn new(client: Client, directory: &Path, bridge: PathBuf) -> Self {
        Self {
            client,
            directory: directory.to_owned(),
            bridge,
        }
    }

    pub fn from_env(client: Client, bridge: PathBuf) -> Result<Self, String> {
        let (variable, default) = match client {
            Client::Codex => ("CODEX_HOME", ".codex"),
            Client::Claude => ("CLAUDE_CONFIG_DIR", ""),
        };
        let directory = std::env::var_os(variable)
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(default)))
            .ok_or_else(|| format!("{variable} or HOME must be set"))?;
        Ok(Self::new(client, &directory, bridge))
    }

    pub fn path(&self) -> PathBuf {
        self.directory.join(self.client.file_name())
    }

    /// Retain existing settings and tool policies, rejecting conflicting registrations.
    /// Returns whether configuration was written.
    pub fn install(&self) -> Result<bool, String> {
        std::fs::create_dir_all(&self.directory).map_err(|error| error.to_string())?;
        // A configuration directory may be a symlink into the user's dotfiles checkout.
        let directory = self
            .directory
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let configuration = Configuration::new(self.client, &self.bridge)?;
        ConfigFile::install(
            &directory,
            &directory.join(self.client.file_name()),
            &configuration,
        )
    }
}

#[cfg(test)]
#[path = "user.tests.rs"]
mod tests;
