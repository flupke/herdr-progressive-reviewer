//! The hook that a real Claude Code runs in the stand-in's place, for the run-ahead contract
//! check: it reports what the agent does on the event socket, as the stand-in agent reports it,
//! so that one test body waits on the same events whichever agent runs in the pane.

use std::io::Read as _;

use review_test_support::stand_in::{StandInConnection, StandInEvent, StandInRole};

/// Reports the event of Claude Code's hook on standard input: a submitted prompt, which a hook
/// sees as it is submitted, as [`StandInEvent::PromptReceived`]; a session the agent resumed as
/// [`StandInEvent::ResumeReceived`] then [`StandInEvent::SessionReported`]; a session it started
/// as [`StandInEvent::SessionReported`]. A subagent's session is not the agent's, nor is a
/// fork's: a fork runs the agent's hooks, since it inherits its configuration.
pub(crate) fn run() {
    let mut payload = String::new();
    std::io::stdin().read_to_string(&mut payload).unwrap();
    let event: serde_json::Value = serde_json::from_str(&payload).unwrap();
    let field = |name: &str| event[name].as_str().unwrap_or_default().to_owned();
    let events = match field("hook_event_name").as_str() {
        _ if in_a_fork() => Vec::new(),
        "UserPromptSubmit" => vec![StandInEvent::PromptReceived {
            // The agent reads the turn's files as it reads the prompt that names them.
            text: review_test_support::turn::with_turn(&claude_hooks::unwrap_pastes(&field(
                "prompt",
            ))),
        }],
        "SessionStart" if event.get("agent_id").is_none() => {
            let (session, start) = (field("session_id"), field("source"));
            let resumed = (start == "resume").then(|| StandInEvent::ResumeReceived {
                session: session.clone(),
            });
            resumed
                .into_iter()
                .chain([StandInEvent::SessionReported { session, start }])
                .collect()
        }
        _ => Vec::new(),
    };
    let (report, _) = StandInConnection::from_env(StandInRole::Agent).expect("the event socket");
    for event in &events {
        report.report(event);
    }
}

/// Whether the hook runs under a fork: a process among its ancestors has `--fork-session` in
/// its command line.
fn in_a_fork() -> bool {
    let mut pid = std::process::id().to_string();
    while let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
        let Some(parent) = status
            .lines()
            .find_map(|line| line.strip_prefix("PPid:"))
            .map(str::trim)
        else {
            return false;
        };
        if parent == "0" {
            return false;
        }
        parent.clone_into(&mut pid);
        let command_line = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if command_line
            .split(|byte| *byte == 0)
            .any(|argument| argument == b"--fork-session")
        {
            return true;
        }
    }
    false
}
