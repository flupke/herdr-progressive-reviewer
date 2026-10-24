//! What [`crate::ClaudeForks`] keeps of a fork and of where forks start, in run-ahead's
//! [`HostFields`]. A fork's record holds `transcripts` and `process` at its top level, as
//! records did before the host's fields had a place of their own.

use std::path::{Path, PathBuf};

use agent_fork::{ForkCommand, ProcessStamp};
use review_run_ahead::HostFields;

/// Where Claude Code keeps the transcripts of the agent and of its forks.
const TRANSCRIPTS: &str = "transcripts";
/// A fork's process, once it started.
const PROCESS: &str = "process";
/// The agent's own command, without what picks its session or its interactive mode.
const COMMAND: &str = "command";

/// The fields of a point whose transcripts are under `transcripts`, of an agent that runs
/// `command`.
pub(crate) fn point(transcripts: &Path, command: &ForkCommand) -> HostFields {
    let mut fields = HostFields::default();
    fields.set(TRANSCRIPTS, transcripts);
    fields.set(COMMAND, command);
    fields
}

/// The fields of a fork whose transcript is under `transcripts`.
pub(crate) fn fork(transcripts: PathBuf) -> HostFields {
    let mut fields = HostFields::default();
    fields.set(TRANSCRIPTS, transcripts);
    fields
}

/// The fields a fork's process adds once it started.
pub(crate) fn started(process: ProcessStamp) -> HostFields {
    let mut fields = HostFields::default();
    fields.set(PROCESS, process);
    fields
}

pub(crate) fn transcripts(fields: &HostFields) -> Option<PathBuf> {
    fields.get(TRANSCRIPTS)
}

pub(crate) fn command(fields: &HostFields) -> Option<ForkCommand> {
    fields.get(COMMAND)
}

/// The process of the fork whose fields are `fields`, once it started.
pub fn process(fields: &HostFields) -> Option<ProcessStamp> {
    fields.get(PROCESS)
}

#[cfg(test)]
#[path = "fields.tests.rs"]
mod tests;
