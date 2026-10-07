//! The `PreToolUse` hook every fork runs: a fork reads code and submits its turn to the
//! reviewer, and does nothing else. The hook refuses every tool that writes, every MCP tool but
//! the two submits (whatever access value from its history a fork would give them), and every
//! shell command that could write or run another program. A hook keeps the fork's tool list the
//! same as the agent's, and so its prompt cache, where disallowing tools would change it.

use std::io::Read;

use commands::Refusal;
use shell::ShellError;

mod commands;
mod sed;
mod shell;

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
/// The reviewer's tools a fork may call: the two that submit a turn.
pub const SUBMITS: &[&str] = &[
    "mcp__herdr_reviewer__submit_question",
    "mcp__herdr_reviewer__submit_conclusion",
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

/// Why the shell command `command` is refused; `None` when it only reads: commands of the
/// list, alone or piped, with no shell form outside quotes.
fn command_refusal(command: &str) -> Option<String> {
    let refusal = match shell::split_pipe(command) {
        Ok(pipe) => pipe.iter().find_map(|words| commands::refusal(words))?,
        Err(error) => return Some(shell_refusal(command, &error)),
    };
    Some(match refusal {
        Refusal::NotReading => format!(
            "`{command}` is not allowed: this session only reads code (jj and git log, diff, \
             show; jj file show and list; rg; cat; sed -n with p; ls and the like), one \
             command or a pipe."
        ),
        Refusal::Snapshot => format!(
            "`{command}` is not allowed: run jj with --ignore-working-copy, as in \
             `jj --ignore-working-copy log`, so that it records no operation."
        ),
    })
}

fn shell_refusal(command: &str, error: &ShellError) -> String {
    let reason = match error {
        ShellError::Form('`') => "a backtick outside single quotes".to_owned(),
        ShellError::Form('$') => "`$` outside single quotes".to_owned(),
        ShellError::Form(form) => format!("`{form}` outside quotes"),
        ShellError::LeadingGlob(glob) => {
            format!("`{glob}` outside quotes at the start of a word or in an option")
        }
        ShellError::Control => "a newline or another control character".to_owned(),
        ShellError::Unclosed => "an unclosed quote".to_owned(),
        ShellError::Empty => "an empty command".to_owned(),
    };
    format!(
        "`{command}` is not allowed: it holds {reason}. This session runs one reading command \
         or a pipe, with no list, substitution or redirection but 2>/dev/null and 2>&1; quote \
         a pattern that holds one of these characters."
    )
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
