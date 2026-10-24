//! The `PreToolUse` hook every fork runs: a fork reads code and submits its turn to the
//! reviewer, and does nothing else. The hook refuses every tool that writes, every MCP tool but
//! the two submits (whatever access value from its history a fork would give them), and every
//! shell command that could write or run another program. A hook keeps the fork's tool list the
//! same as the agent's, and so its prompt cache, where disallowing tools would change it.

use std::io::Read;

pub use fork_guard::SUBMITS;
use fork_guard::command_refusal;

/// The tools a fork may use, besides the shell (checked command by command) and the two
/// submits: those that read, search, plan or start a subagent, whose calls this hook checks too.
const READERS: &[&str] = &[
    "Read",
    "Grep",
    "Glob",
    "LS",
    "NotebookRead",
    "Agent",
    "Task",
    "TodoWrite",
    "ToolSearch",
    "WebFetch",
    "WebSearch",
    "BashOutput",
];

/// Why the tool call `input`, the hook's JSON, is refused; `None` when it may run.
fn refusal(input: &serde_json::Value) -> Option<String> {
    let tool = input.get("tool_name")?.as_str()?;
    if tool.starts_with("mcp__") && !SUBMITS.contains(&tool) {
        return Some(format!(
            "{tool} is not allowed here: only submit_question and submit_conclusion."
        ));
    }
    if tool != "Bash" && !READERS.contains(&tool) && !SUBMITS.contains(&tool) {
        return Some(format!(
            "{tool} is not allowed: this session reads code and writes nothing."
        ));
    }
    if tool == "Bash" {
        let command = input
            .pointer("/tool_input/command")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        return command_refusal(command);
    }
    None
}

/// The hook's entry point: reads the tool call on standard input, then exits 2 with the reason
/// on standard error to refuse it, or 0 to let it run.
pub fn run_guard() -> i32 {
    let mut text = String::new();
    let _ = std::io::stdin().read_to_string(&mut text);
    let Ok(input) = serde_json::from_str::<serde_json::Value>(&text) else {
        eprintln!("The reviewer's guard could not read the tool call.");
        return 2;
    };
    match refusal(&input) {
        Some(reason) => {
            eprintln!("{reason}");
            2
        }
        None => 0,
    }
}

#[cfg(test)]
#[path = "guard.tests.rs"]
mod tests;
