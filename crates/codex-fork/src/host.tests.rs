use std::path::Path;

use agent_hooks::HookDirectory;

use super::*;

/// The Codex agent of the pane `w:p1`, as Herdr reports it.
fn codex_agent() -> Agent {
    serde_json::from_value(json!({
        "pane_id": "w:p1",
        "tab_id": "tab",
        "workspace_id": "workspace",
        "agent": "codex",
        "agent_status": "idle",
    }))
    .unwrap()
}

#[test]
fn an_agent_whose_session_the_reviewer_does_not_know_yet_is_not_taken_for_one_without_hooks() {
    let runtime = tempfile::tempdir().unwrap();
    let hooks = Arc::new(
        AgentHooks::listen(&HookDirectory::for_server(
            runtime.path(),
            Path::new("/run/herdr.sock"),
        ))
        .unwrap(),
    );
    // No Herdr answers: the agent's daemon is unknown, as is its process.
    let herdr = HerdrClient::new(
        runtime.path().join("no-herdr.sock"),
        "test".into(),
        runtime.path().to_owned(),
    );
    let forks = CodexForks::new(
        herdr,
        RunAheadLog::default(),
        ForkWaits::default(),
        Some(Arc::clone(&hooks)),
    );
    let agent = codex_agent();

    // As in a reopened reviewer, before the agent called the reviewer's tools: it may run a
    // fork's session, which a settle must not take for one the agent left.
    assert_eq!(forks.unhooked(&agent), None);

    // A session whose prompts never ran the plugin's hook.
    hooks.name_session(&agent.pane_id, "thread-without-the-hook");
    assert!(forks.unhooked(&agent).is_some());
}
