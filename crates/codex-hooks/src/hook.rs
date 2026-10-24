//! What the plugin's hook runs.

use std::io::{Read, Write};
use std::time::Duration;

use agent_hooks::{AgentEvent, HookRoot, Report};
use serde::Deserialize;

/// The subcommand of `reviewer-control` that the plugin's hook runs.
pub const SUBCOMMAND: &str = "codex-hook";

/// How long a submitted prompt waits for the reviewers' answer before it goes on: Codex waits
/// for the hook before it takes the prompt.
const ASK_LIMIT: Duration = Duration::from_millis(500);

/// Runs the hook whose event Codex writes on standard input, and prints the decision Codex
/// reads, if one is made. It ignores the `HERDR_*` variables: under the daemon, they name the
/// pane and the Herdr server of the terminal that started it.
pub fn run_hook() {
    let mut payload = Vec::new();
    // Read whole, so that Codex never writes to a closed pipe.
    if std::io::stdin().read_to_end(&mut payload).is_err() {
        return;
    }
    let Some(root) = HookRoot::from_env() else {
        return;
    };
    if let Some(decision) = hook(&root, &payload) {
        let _ = std::io::stdout().write_all(decision.as_bytes());
    }
}

/// The events of Codex's hooks that the reviewer reads, with the fields it reads.
#[derive(Deserialize)]
#[serde(tag = "hook_event_name")]
enum Payload {
    UserPromptSubmit {
        /// The session's thread ID.
        session_id: String,
        prompt: String,
    },
    PreToolUse {
        session_id: String,
        tool_name: String,
        #[serde(default)]
        tool_input: serde_json::Value,
    },
    /// An event the plugin does not register.
    #[serde(other)]
    Other,
}

/// Answers the event `payload` with the reviewers of `root`; returns the decision Codex reads,
/// when one is made.
fn hook(root: &HookRoot, payload: &[u8]) -> Option<String> {
    match serde_json::from_slice::<Payload>(payload).ok()? {
        Payload::UserPromptSubmit { session_id, prompt } => {
            prompt_submitted(root, session_id, prompt)
        }
        Payload::PreToolUse {
            session_id,
            tool_name,
            tool_input,
        } => {
            // Every tool call of every session of the user comes here: one file check.
            if !root.is_codex_fork(&session_id) {
                return None;
            }
            let reason = crate::guard::refusal(&tool_name, &tool_input)?;
            Some(
                serde_json::json!({"hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": reason,
                }})
                .to_string(),
            )
        }
        Payload::Other => None,
    }
}

/// Records that the session `session_id` runs the hook, and asks the reviewers of `root`
/// whether they block its prompt `prompt`; returns the decision Codex reads, when one does.
fn prompt_submitted(root: &HookRoot, session_id: String, prompt: String) -> Option<String> {
    let _ = root.record_codex_session(&session_id);
    let report = Report {
        pane: None,
        event: AgentEvent::PromptSubmitted {
            prompt,
            session: Some(session_id),
        },
    };
    let reason = root.ask(&report, ASK_LIMIT)?;
    Some(serde_json::json!({"decision": "block", "reason": reason}).to_string())
}

#[cfg(test)]
#[path = "hook.tests.rs"]
mod tests;
