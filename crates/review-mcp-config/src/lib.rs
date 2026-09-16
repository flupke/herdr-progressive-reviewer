//! Registration of the reviewer's MCP bridge in native clients.

mod document;
mod file;
mod user;

use std::str::FromStr;

pub use user::UserConfig;

/// A native client supported by the reviewer.
#[derive(Clone, Copy, Debug)]
pub enum Client {
    Codex,
    Claude,
}

impl Client {
    pub const ALL: [Self; 2] = [Self::Codex, Self::Claude];

    fn file_name(self) -> &'static str {
        match self {
            Self::Codex => "config.toml",
            Self::Claude => ".claude.json",
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

#[cfg(test)]
mod tests;
