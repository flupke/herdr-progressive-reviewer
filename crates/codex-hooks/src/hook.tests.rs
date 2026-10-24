use std::path::Path;
use std::time::Duration;

use agent_hooks::{AgentHooks, Heard, HookDirectory, HookedAgent};
use herdr_client::protocol::{Agent, PaneId};

use super::*;

/// How long a test waits for an event that a failing test never gets.
const GUARD: Duration = Duration::from_secs(30);

/// Codex's `UserPromptSubmit` event, as 0.162.1 writes it, of `prompt` on the session
/// `session`.
fn prompt_submit(session: &str, prompt: &str) -> String {
    serde_json::json!({
        "session_id": session,
        "turn_id": "turn-1",
        "transcript_path": null,
        "cwd": "/repo",
        "hook_event_name": "UserPromptSubmit",
        "model": "gpt-6.1-sol",
        "permission_mode": "default",
        "prompt": prompt,
    })
    .to_string()
}

/// A reviewer of a Herdr server of the user, and the user's root of every server.
fn reviewer() -> (tempfile::TempDir, HookRoot, AgentHooks) {
    let runtime = tempfile::tempdir().unwrap();
    let directory = HookDirectory::for_server(runtime.path(), Path::new("/run/herdr.sock"));
    let hooks = AgentHooks::listen(&directory).unwrap();
    (runtime, directory.root(), hooks)
}

/// The Codex agent of the pane `w:p1`, as Herdr reports it.
fn codex_agent() -> Agent {
    serde_json::from_value(serde_json::json!({
        "pane_id": "w:p1",
        "tab_id": "tab",
        "workspace_id": "workspace",
        "agent": "codex",
        "agent_status": "idle",
    }))
    .unwrap()
}

#[test]
fn the_prompt_the_reviewer_typed_goes_on_is_heard_and_its_session_runs_the_hook() {
    let (_runtime, root, hooks) = reviewer();
    let port = herdr_client::memory::InMemoryAgents::default();
    hooks.name_session(&PaneId("w:p1".into()), "thread-1");
    assert_eq!(hooks.hooked(&port, &codex_agent()), None);
    let codex = HookedAgent::Codex {
        session: "thread-1".into(),
    };
    let expectation = hooks.expect_prompt(&codex, "Reply in review r1", "blocked");

    let decision = hook(
        &root,
        prompt_submit("thread-1", "Reply in review r1").as_bytes(),
    );

    assert_eq!(decision, None);
    assert_eq!(expectation.wait(GUARD), Some(Heard::Submitted));
    assert_eq!(hooks.hooked(&port, &codex_agent()), Some(codex));
}

#[test]
fn text_left_in_the_input_box_that_joined_the_typed_prompt_is_blocked_with_the_reason() {
    let (_runtime, root, hooks) = reviewer();
    let codex = HookedAgent::Codex {
        session: "thread-1".into(),
    };
    let expectation = hooks.expect_prompt(&codex, "Reply in review r1", "empty the box");

    let decision = hook(
        &root,
        prompt_submit("thread-1", "my wordsReply in review r1").as_bytes(),
    );

    let decision: serde_json::Value = serde_json::from_str(&decision.unwrap()).unwrap();
    assert_eq!(
        decision,
        serde_json::json!({"decision": "block", "reason": "empty the box"})
    );
    assert_eq!(
        expectation.wait(Duration::ZERO),
        Some(Heard::Joined {
            left: "my words".into()
        })
    );
}

#[test]
fn another_event_is_neither_reported_nor_recorded() {
    let (_runtime, root, hooks) = reviewer();
    let port = herdr_client::memory::InMemoryAgents::default();
    hooks.name_session(&PaneId("w:p1".into()), "thread-1");
    let codex = HookedAgent::Codex {
        session: "thread-1".into(),
    };
    let expectation = hooks.expect_resume(&codex, "fork", "blocked");
    let mut stop: serde_json::Value =
        serde_json::from_str(&prompt_submit("thread-1", "hello")).unwrap();
    stop["hook_event_name"] = "Stop".into();

    assert_eq!(hook(&root, stop.to_string().as_bytes()), None);

    assert_eq!(expectation.wait(Duration::ZERO), None);
    assert_eq!(hooks.hooked(&port, &codex_agent()), None);
}

/// Codex's `PreToolUse` event, as 0.162.1 writes it, of the tool `tool` called with `input` on
/// the session `session`.
fn tool_use(session: &str, tool: &str, input: &serde_json::Value) -> String {
    serde_json::json!({
        "session_id": session,
        "turn_id": "turn-1",
        "transcript_path": null,
        "cwd": "/repo",
        "hook_event_name": "PreToolUse",
        "model": "gpt-6.1-sol",
        "permission_mode": "default",
        "tool_name": tool,
        "tool_input": input,
        "tool_use_id": "call-1",
    })
    .to_string()
}

#[test]
fn a_fork_s_session_is_denied_every_tool_but_reading_and_submitting() {
    let (_runtime, root, _hooks) = reviewer();
    root.mark_codex_fork("fork-1").unwrap();

    let denied = hook(
        &root,
        tool_use("fork-1", "mcp__slack__send_message", &serde_json::json!({})).as_bytes(),
    )
    .expect("a decision");
    let decision: serde_json::Value = serde_json::from_str(&denied).unwrap();
    assert_eq!(
        decision["hookSpecificOutput"]["permissionDecision"], "deny",
        "{decision}"
    );
    assert_eq!(
        decision["hookSpecificOutput"]["hookEventName"],
        "PreToolUse"
    );
    assert_eq!(
        hook(
            &root,
            tool_use(
                "fork-1",
                "mcp__herdr_reviewer__submit_question",
                &serde_json::json!({})
            )
            .as_bytes()
        ),
        None
    );
}

#[test]
fn any_other_session_runs_every_tool() {
    let (_runtime, root, _hooks) = reviewer();
    root.mark_codex_fork("fork-1").unwrap();

    let input = serde_json::json!({"command": "rm -rf build"});
    assert_eq!(
        hook(&root, tool_use("thread-1", "Bash", &input).as_bytes()),
        None
    );
}
