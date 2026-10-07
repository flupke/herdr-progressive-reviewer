//! The reviewer's Claude Code plugin, and what its hooks run.
//!
//! The plugin (`plugin/`, listed by the marketplace of this directory, which `make install`
//! adds) runs `reviewer-control agent-hook` on each hook it registers, with Claude Code's event
//! on its standard input. In a Herdr pane, that hands the event to the reviewers of the pane's
//! Herdr server ([`agent_hooks`]); anywhere else, it does nothing. Claude Code reads its hooks
//! when it starts: an agent started before the plugin was installed runs without them.

use std::io::Read;

use agent_hooks::{AgentEvent, HookDirectory, Report, SessionSource};
use herdr_client::protocol::PaneId;
use serde::Deserialize;

/// The subcommand of `reviewer-control` that each hook of the plugin runs.
pub const SUBCOMMAND: &str = "agent-hook";

/// Runs the hook whose event Claude Code writes on standard input, for the Herdr pane this
/// process runs in, if any.
pub fn run_hook() {
    let mut payload = Vec::new();
    // Read whole, so that Claude Code never writes to a closed pipe.
    if std::io::stdin().read_to_end(&mut payload).is_err() {
        return;
    }
    let pane = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|pane| !pane.is_empty());
    let (Some(pane), Some(directory)) = (pane, HookDirectory::from_env()) else {
        return;
    };
    hook(&PaneId(pane), &directory, &payload);
}

/// The fields of Claude Code's hook events that the reviewer reads.
#[derive(Deserialize)]
struct Payload {
    hook_event_name: String,
    session_id: String,
    /// Why the session started, for `SessionStart`.
    #[serde(default)]
    source: Option<SessionSource>,
    /// The subagent the event belongs to, if it belongs to one.
    #[serde(default)]
    agent_id: Option<String>,
}

/// Hands the event `payload` of the agent of `pane` to the reviewers of `directory`.
fn hook(pane: &PaneId, directory: &HookDirectory, payload: &[u8]) {
    let Ok(payload) = serde_json::from_slice::<Payload>(payload) else {
        return;
    };
    // A subagent's events are not the agent's.
    if payload.agent_id.is_some() {
        return;
    }
    if payload.hook_event_name == "SessionStart" {
        directory.tell(&Report {
            pane: pane.clone(),
            event: AgentEvent::SessionStarted {
                session: payload.session_id,
                source: payload.source.unwrap_or(SessionSource::Other),
            },
        });
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
