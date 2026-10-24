//! The guard of a Codex fork: in a session a reviewer marked as a fork's, the plugin's
//! `PreToolUse` hook lets the fork read code and submit its turn, and refuses every other tool,
//! as Claude Code's forks' guard does. Codex asks no approval before an MCP tool the user set
//! to approve, nor before one its server marks read-only: this hook refuses those too, an MCP
//! server a plugin brings included. In any other session, the hook lets every tool run.

use fork_guard::{SUBMITS, command_refusal};
use serde_json::Value;

/// The tools a fork may use, besides the shell (checked command by command) and the two
/// submits, as Codex 0.162.1 names them to the hook: those that read or wait.
const READERS: &[&str] = &["view_image", "get_goal", "clockcurr_time", "clocksleep"];

/// Why the fork's call of the tool `tool` with `input` is refused; `None` when it may run.
pub(crate) fn refusal(tool: &str, input: &Value) -> Option<String> {
    if SUBMITS.contains(&tool) || READERS.contains(&tool) {
        return None;
    }
    if tool == "Bash" {
        let command = input
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return command_refusal(command);
    }
    Some(if tool.starts_with("mcp__") {
        format!("{tool} is not allowed here: only submit_question and submit_conclusion.")
    } else {
        format!("{tool} is not allowed: this session reads code and writes nothing.")
    })
}

#[cfg(test)]
#[path = "guard.tests.rs"]
mod tests;
