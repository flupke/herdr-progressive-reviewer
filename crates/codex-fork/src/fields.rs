//! What [`crate::CodexForks`] keeps of a fork and of where forks start, in run-ahead's
//! [`HostFields`]: the daemon that runs them, so that a reopened reviewer deletes a stopped
//! one's forks there; the turn a fork takes, which a discard interrupts; and the agent's own
//! settings, which a fork's session gets back before the agent resumes it.

use std::path::PathBuf;

use codex_daemon::Daemon;
use review_run_ahead::HostFields;
use serde_json::Value;

/// The control socket of the daemon that runs the agent's session and its forks.
const DAEMON: &str = "codex_daemon";
/// The turn a fork takes, once it started.
const TURN: &str = "codex_turn";
/// The settings of the agent's session that a fork's session takes back, as
/// `thread/settings/update` takes them.
const SETTINGS: &str = "codex_settings";

/// The fields of a point whose forks start on `daemon`.
pub(crate) fn point(daemon: &Daemon) -> HostFields {
    let mut fields = HostFields::default();
    fields.set(DAEMON, daemon.socket());
    fields
}

/// The fields of a fork that runs on `daemon`, whose session takes back the agent's
/// `settings` before the agent resumes it.
pub(crate) fn fork(daemon: &Daemon, settings: &Value) -> HostFields {
    let mut fields = point(daemon);
    fields.set(SETTINGS, settings);
    fields
}

/// The fields a fork adds once it took the turn `turn`.
pub(crate) fn started(turn: &str) -> HostFields {
    let mut fields = HostFields::default();
    fields.set(TURN, turn);
    fields
}

pub(crate) fn daemon(fields: &HostFields) -> Option<Daemon> {
    fields.get::<PathBuf>(DAEMON).map(Daemon::at)
}

/// The turn the fork whose fields are `fields` took, once it started.
pub fn turn(fields: &HostFields) -> Option<String> {
    fields.get(TURN)
}

pub(crate) fn settings(fields: &HostFields) -> Option<Value> {
    fields.get(SETTINGS)
}

/// Whether `fields` are those of a Codex fork, or of where Codex forks start.
pub fn owns(fields: &HostFields) -> bool {
    fields.has(DAEMON)
}
