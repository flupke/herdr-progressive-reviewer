//! The rules of a fork's guard: a fork of the agent's session reads code and submits its turn
//! to the reviewer, and does nothing else. This crate holds what the guards of every agent
//! share: the reviewer's two submit tools, and which shell commands only read. A shell command
//! is refused when it could write or run another program, or when it holds a shell form, such
//! as a redirection or a substitution, that would make the shell run something else than the
//! guard read. Claude Code's guard (`claude-fork`) and Codex's (`codex-hooks`) add the agent's
//! own tools.

use commands::Refusal;
use shell::ShellError;

mod commands;
mod sed;
mod shell;

/// The reviewer's tools a fork may call, as Claude Code and Codex name them: the two that
/// submit a turn.
pub const SUBMITS: &[&str] = &[
    "mcp__herdr_reviewer__submit_question",
    "mcp__herdr_reviewer__submit_conclusion",
];

/// Why the shell command `command` is refused; `None` when it only reads: commands of the
/// list, alone or piped, with no shell form outside quotes.
pub fn command_refusal(command: &str) -> Option<String> {
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

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
