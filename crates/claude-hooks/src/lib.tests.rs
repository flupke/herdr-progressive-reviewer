use std::path::Path;
use std::time::Duration;

use agent_hooks::{AgentHooks, Heard};
use serde_json::Value;

use super::*;

/// How long a test waits for an event that a failing test never gets.
const GUARD: Duration = Duration::from_secs(30);

/// A reviewer listening in a new directory.
fn reviewer() -> (tempfile::TempDir, HookDirectory, AgentHooks) {
    let runtime = tempfile::tempdir().unwrap();
    let directory = HookDirectory::for_server(runtime.path(), Path::new("/run/herdr.sock"));
    let hooks = AgentHooks::listen(&directory).unwrap();
    (runtime, directory, hooks)
}

/// Claude Code's `SessionStart` event, as 2.1.292 writes it, of `session` from `source`.
fn session_start(session: &str, source: &str) -> String {
    format!(
        r#"{{"session_id":"{session}","transcript_path":"/home/u/.claude/projects/-repo/{session}.jsonl","cwd":"/repo","hook_event_name":"SessionStart","source":"{source}","model":"claude-opus-5-5"}}"#
    )
}

#[test]
fn a_session_claude_code_resumed_reaches_the_reviewer_that_expects_it() {
    let (_runtime, directory, hooks) = reviewer();
    let pane = PaneId("w:p1".into());
    let expectation = hooks.expect_resume(&pane, "fork", "blocked");

    hook(
        &pane,
        &directory,
        session_start("fork", "resume").as_bytes(),
    );

    assert_eq!(expectation.wait(GUARD), Some(Heard::Resumed));
}

#[test]
fn a_subagent_s_session_is_not_the_agent_s() {
    let (_runtime, directory, hooks) = reviewer();
    let pane = PaneId("w:p1".into());
    let expectation = hooks.expect_resume(&pane, "fork", "blocked");
    let marker = hooks.expect_resume(&PaneId("w:p9".into()), "marker", "blocked");
    let mut subagent: Value = serde_json::from_str(&session_start("fork", "resume")).unwrap();
    subagent["agent_id"] = "a1".into();

    hook(&pane, &directory, subagent.to_string().as_bytes());
    // The reviewer takes the hooks one after another: it took the subagent's before this.
    hook(
        &PaneId("w:p9".into()),
        &directory,
        session_start("marker", "resume").as_bytes(),
    );

    assert_eq!(marker.wait(GUARD), Some(Heard::Resumed));
    assert_eq!(expectation.wait(Duration::ZERO), None);
}

/// Claude Code's `UserPromptSubmit` event, as 2.1.292 writes it, of `prompt`.
fn prompt_submit(prompt: &str) -> String {
    serde_json::json!({
        "session_id": "session",
        "transcript_path": "/home/u/.claude/projects/-repo/session.jsonl",
        "cwd": "/repo",
        "permission_mode": "default",
        "hook_event_name": "UserPromptSubmit",
        "prompt": prompt,
    })
    .to_string()
}

#[test]
fn a_prompt_submitted_while_the_reviewer_expects_a_session_is_blocked_with_its_reason() {
    let (_runtime, directory, hooks) = reviewer();
    let pane = PaneId("w:p1".into());
    let expectation = hooks.expect_resume(&pane, "fork", "A draft met the reviewer's /resume.");

    let decision = hook(
        &pane,
        &directory,
        prompt_submit("half a thought /resume fork").as_bytes(),
    );

    let decision: Value = serde_json::from_str(&decision.unwrap()).unwrap();
    assert_eq!(
        decision,
        serde_json::json!({"decision": "block", "reason": "A draft met the reviewer's /resume."})
    );
    assert_eq!(
        expectation.wait(Duration::ZERO),
        Some(Heard::Blocked {
            prompt: "half a thought /resume fork".into()
        })
    );
}

#[test]
fn a_prompt_the_reviewer_does_not_block_goes_on_without_a_decision() {
    let (_runtime, directory, _hooks) = reviewer();

    let decision = hook(
        &PaneId("w:p1".into()),
        &directory,
        prompt_submit("Reply with ok").as_bytes(),
    );

    assert_eq!(decision, None);
}

/// The JSON file at `path` under this crate.
fn manifest(path: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn each_hook_of_the_plugin_runs_the_agent_hook_of_the_installed_reviewer() {
    let marketplace = manifest(".claude-plugin/marketplace.json");
    let plugin = manifest("plugin/.claude-plugin/plugin.json");
    let hooks = manifest("plugin/hooks/hooks.json");

    let listed = &marketplace["plugins"][0];
    assert_eq!(listed["name"], plugin["name"]);
    assert_eq!(listed["source"], "./plugin");
    assert_eq!(plugin["userConfig"]["control"]["required"], true);
    let commands: Vec<&Value> = hooks["hooks"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|matchers| matchers.as_array().unwrap())
        .flat_map(|matcher| matcher["hooks"].as_array().unwrap())
        .collect();
    assert!(!commands.is_empty());
    for command in commands {
        // Exec form: Claude Code refuses a user option in a command a shell would parse.
        assert_eq!(command["command"], "${user_config.control}");
        assert_eq!(command["args"], serde_json::json!([SUBCOMMAND]));
    }
}
