//! The `PreToolUse` hook every fork runs: a fork reads code and submits its turn to the
//! reviewer, and does nothing else. The hook refuses every tool that writes, every MCP tool but
//! the two submits (whatever access value from its history a fork would give them), and every
//! shell command that could write or run another program. A hook keeps the fork's tool list the
//! same as the agent's, and so its prompt cache, where disallowing tools would change it.

use std::io::Read;

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
/// Commands a fork may run, by name, each with the subcommands it may run when it has some.
const COMMANDS: &[(&str, &[&str])] = &[
    (
        "jj",
        &[
            "log",
            "diff",
            "show",
            "st",
            "status",
            "evolog",
            "interdiff",
            "root",
        ],
    ),
    (
        "git",
        &[
            "log",
            "diff",
            "show",
            "status",
            "blame",
            "ls-files",
            "cat-file",
            "rev-parse",
            "grep",
            "merge-base",
        ],
    ),
    ("rg", &[]),
    ("grep", &[]),
    ("ls", &[]),
    ("cat", &[]),
    ("head", &[]),
    ("tail", &[]),
    ("wc", &[]),
    ("find", &[]),
    ("pwd", &[]),
    ("nl", &[]),
    ("cut", &[]),
    ("diff", &[]),
];
/// What a shell command may not hold: redirections, command lists, substitutions.
const SHELL_FORMS: &[&str] = &[">", ";", "&", "`", "$", "<(", "\n", "\r"];
/// Options of the listed commands that write a file, run another program, or change the
/// configuration that could run one.
const WRITING_OPTIONS: &[&str] = &[
    "--config",
    "--config-file",
    "--config-toml",
    "--tool",
    "--pre",
    "--output",
    "-O",
    "--open-files-in-pager",
    "--ext-diff",
    "--textconv",
    "-exec",
    "-execdir",
    "-delete",
    "-ok",
    "-okdir",
    "-fprint",
    "-fprint0",
    "-fprintf",
    "-fls",
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
        return (!reads_only(command)).then(|| {
            format!(
                "`{command}` is not allowed: this session only reads code (jj and git log, \
                 diff, show; rg; cat; ls and the like), one command or a pipe, no redirection."
            )
        });
    }
    None
}

/// Whether the shell command `command` only reads: commands of the list, piped, with nothing
/// that redirects, chains or substitutes.
fn reads_only(command: &str) -> bool {
    if SHELL_FORMS.iter().any(|form| command.contains(form)) {
        return false;
    }
    command.split('|').all(|part| {
        let mut words = part.split_whitespace();
        let Some(name) = words.next() else {
            return false;
        };
        let Some((_, subcommands)) = COMMANDS.iter().find(|(listed, _)| *listed == name) else {
            return false;
        };
        let writes = part.split_whitespace().any(|word| {
            WRITING_OPTIONS
                .iter()
                .any(|option| word == *option || word.starts_with(&format!("{option}=")))
                || (word.starts_with("-O") || word.starts_with("--pre")) && word.len() > 2
        });
        if writes {
            return false;
        }
        subcommands.is_empty()
            || words
                .find(|word| !word.starts_with('-'))
                .is_some_and(|subcommand| subcommands.contains(&subcommand))
    })
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
