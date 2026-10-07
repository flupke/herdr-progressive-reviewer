//! `reviewer-control fork-guard`, a fork's `PreToolUse` hook, answers Claude Code as its hooks
//! expect: exit code 2 with the reason on standard error blocks the tool call; exit code 0 lets
//! it run. Any other code would let a refused call run.

use std::io::Write;
use std::process::{Command, Stdio};

/// The guard's exit code and standard error for the hook input `input`.
fn guard(input: &str) -> (Option<i32>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_reviewer-control"))
        .arg("fork-guard")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.code(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

fn bash(command: &str) -> String {
    serde_json::json!({"tool_name": "Bash", "tool_input": {"command": command}}).to_string()
}

#[test]
fn the_guard_blocks_a_refused_call_and_lets_a_read_run() {
    assert_eq!(
        guard(&bash("rg -n '&self' src | head")),
        (Some(0), String::new())
    );
    let (code, reason) = guard(&bash("jj log"));
    assert_eq!(code, Some(2));
    assert!(reason.contains("--ignore-working-copy"), "{reason}");
    let (code, reason) = guard(&bash("rg x; rm -rf y"));
    assert_eq!(code, Some(2));
    assert!(reason.contains("`;`"), "{reason}");
    assert_eq!(guard("not json").0, Some(2));
}
