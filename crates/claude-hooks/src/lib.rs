//! The reviewer's Claude Code plugin, and what its hooks run.
//!
//! The plugin (`plugin/`, listed by the marketplace of this directory, which `make install`
//! adds) runs `reviewer-control agent-hook` on each hook it registers, with Claude Code's event
//! on its standard input. In a Herdr pane, that hands the event to the reviewers of the pane's
//! Herdr server ([`agent_hooks`]), and a prompt a reviewer blocks does not run; anywhere else,
//! it does nothing. Claude Code reads its hooks when it starts: an agent started before the
//! plugin was installed runs without them.

use std::io::{Read, Write};
use std::time::Duration;

use agent_fork::ProcessStamp;
use agent_hooks::{AgentEvent, HookDirectory, HookedSession, Report, SessionSource};
use herdr_client::protocol::PaneId;
use serde::Deserialize;

/// The subcommand of `reviewer-control` that each hook of the plugin runs.
pub const SUBCOMMAND: &str = "agent-hook";

/// How long a submitted prompt waits for the reviewers' answer before it goes on: Claude Code
/// waits for the hook before it takes the prompt.
const ASK_LIMIT: Duration = Duration::from_millis(500);

/// Runs the hook whose event Claude Code writes on standard input, for the Herdr pane this
/// process runs in, if any, and prints the decision Claude Code reads, if one is made.
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
    if let Some(decision) = hook(&PaneId(pane), &directory, &payload) {
        let _ = std::io::stdout().write_all(decision.as_bytes());
    }
}

/// The events of Claude Code's hooks that the reviewer reads, with the fields it reads.
#[derive(Deserialize)]
#[serde(tag = "hook_event_name")]
enum Payload {
    SessionStart {
        session_id: String,
        #[serde(default)]
        source: SessionSource,
        /// The subagent the session belongs to, if it belongs to one.
        #[serde(default)]
        agent_id: Option<String>,
    },
    UserPromptSubmit {
        prompt: String,
    },
    /// An event the plugin does not register.
    #[serde(other)]
    Other,
}

/// Hands the event `payload` of the agent of `pane` to the reviewers of `directory`; returns
/// the decision Claude Code reads, when a reviewer blocks a prompt.
fn hook(pane: &PaneId, directory: &HookDirectory, payload: &[u8]) -> Option<String> {
    match serde_json::from_slice::<Payload>(payload).ok()? {
        // A subagent's session is not the agent's.
        Payload::SessionStart {
            agent_id: Some(_), ..
        }
        | Payload::Other => None,
        Payload::SessionStart {
            session_id, source, ..
        } => {
            // Claude Code runs the hook itself, without a shell: its parent is the agent.
            let process = ProcessStamp::read(std::os::unix::process::parent_id());
            let started = HookedSession {
                session: session_id.clone(),
                process,
            };
            let _ = directory.record(pane, &started);
            directory.tell(&Report {
                pane: pane.clone(),
                event: AgentEvent::SessionStarted {
                    session: session_id,
                    source,
                },
            });
            None
        }
        Payload::UserPromptSubmit { prompt } => {
            let report = Report {
                pane: pane.clone(),
                event: AgentEvent::PromptSubmitted { prompt },
            };
            let reason = directory.ask(&report, ASK_LIMIT)?;
            Some(serde_json::json!({"decision": "block", "reason": reason}).to_string())
        }
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
