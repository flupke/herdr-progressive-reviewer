use std::fs;
use std::path::{Path, PathBuf};

/// Herdr agent detection rules that the repository keeps for one agent.
///
/// Every test server installs them as a local override, which Herdr prefers
/// to the rules built into its binary, so the fake agents of the tests reach
/// the same states whatever Herdr release runs them.
pub(crate) struct DetectionRules {
    agent: &'static str,
    manifest: &'static str,
}

impl DetectionRules {
    /// The rules for every agent that the tests impersonate.
    pub(crate) const ALL: [Self; 2] = [
        Self {
            agent: "claude",
            manifest: include_str!("../agent-detection/claude.toml"),
        },
        Self {
            agent: "codex",
            manifest: include_str!("../agent-detection/codex.toml"),
        },
    ];

    /// The agent's process name, which Herdr matches to these rules.
    #[cfg(test)]
    pub(crate) fn agent(&self) -> &str {
        self.agent
    }

    /// Where Herdr reads the agent's local override, under the Herdr
    /// configuration directory.
    pub(crate) fn path(&self, herdr_config_directory: &Path) -> PathBuf {
        herdr_config_directory
            .join("agent-detection")
            .join(format!("{}.toml", self.agent))
    }

    /// Write the rules where a server using `herdr_config_directory` reads
    /// them.
    pub(crate) fn install(&self, herdr_config_directory: &Path) {
        let path = self.path(herdr_config_directory);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, self.manifest).unwrap();
    }
}
