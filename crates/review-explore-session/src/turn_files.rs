//! The files of an agent turn: its instructions, written where its prompt says.

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;

use review_explore_runner::TurnInstructions;
use tempfile::TempDir;

/// The files of one turn, in a temporary directory of their own that goes away with them. Each
/// prompt, a fork's included, gets a new directory, and only this user can read it: the files
/// hold the turn's access value.
#[derive(Debug)]
pub(crate) struct TurnFiles {
    _directory: TempDir,
    /// The prompt that names them.
    prompt: String,
}

impl TurnFiles {
    /// Writes the files of `turn`.
    pub(crate) fn write(turn: &TurnInstructions) -> eyre::Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("herdr-review-turn-")
            .permissions(Permissions::from_mode(0o700))
            .tempdir()?;
        for file in turn.files() {
            std::fs::write(directory.path().join(file.name), &file.text)?;
        }
        let prompt = turn.prompt(directory.path());
        Ok(Self {
            _directory: directory,
            prompt,
        })
    }

    /// The prompt that names the files.
    pub(crate) fn prompt(&self) -> &str {
        &self.prompt
    }
}

#[cfg(test)]
#[path = "turn_files.tests.rs"]
mod tests;
