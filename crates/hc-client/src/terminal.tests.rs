use std::path::PathBuf;

use serde_json::{Value, json};

use super::*;

/// Claude Code in a tab of herdr-app, as `hc get` gives it (herdr-core 0.1.17), trimmed.
fn claude_code() -> Value {
    json!({
        "address": "t2q",
        "argv": ["/bin/zsh"],
        "cols": 200,
        "rows": 120,
        "created_ms": 1_793_802_964_920_u64,
        "cwd": "/home/user/src/project",
        "foreground": {
            "argv": ["claude", "--model", "claude-opus-5-5"],
            "cwd": "/home/user/src/project/crates",
            "name": "claude",
            "pid": 907_504,
        },
        "labels": {
            "herdr.space": "sp4221",
            "herdr.tab": "tbf280",
            "where": "elsewhere",
        },
        "name": null,
        "reported": true,
        "screen": "exact",
        "state": "live",
        "status": {"app": "claude-code", "msg": "reading", "state": "working"},
        "turn": 0,
    })
}

fn terminal(value: Value) -> Terminal {
    serde_json::from_value(value).unwrap()
}

#[test]
fn claude_code_in_a_tab_is_an_agent_of_that_tab_by_herdrs_name() {
    let agent = terminal(claude_code()).agent().unwrap();

    assert_eq!(agent.pane_id, PaneId("t2q".into()));
    assert_eq!(agent.tab_id, TabId("tbf280".into()));
    assert_eq!(agent.workspace_id, WorkspaceId("sp4221".into()));
    assert_eq!(agent.agent.as_deref(), Some("claude"));
    assert_eq!(agent.agent_status, AgentStatus::Working);
    assert_eq!(agent.agent_session, None);
    // Where the agent works now, not where its terminal started.
    assert_eq!(
        agent.cwd,
        Some(PathBuf::from("/home/user/src/project/crates"))
    );
}

#[test]
fn a_program_that_never_reported_a_status_is_no_agent() {
    let mut shell = claude_code();
    shell["status"] = Value::Null;

    assert_eq!(terminal(shell).agent(), None);
}

#[test]
fn an_agent_outside_the_tabs_of_herdr_app_is_none_of_the_reviewers() {
    let mut started_by_hand = claude_code();
    started_by_hand["labels"] = json!({"herdr.space": "sp4221"});

    assert_eq!(terminal(started_by_hand).agent(), None);
}

#[test]
fn an_agent_that_hit_an_error_neither_waits_for_a_prompt_nor_works_on_one() {
    let mut failed = claude_code();
    failed["status"]["state"] = json!("error");

    let status = terminal(failed).agent().unwrap().agent_status;

    assert!(!status.waits_for_prompt());
    assert!(!status.started_on_prompt());
}

#[test]
fn a_finished_agent_waits_for_its_next_prompt() {
    let mut finished = claude_code();
    finished["status"]["state"] = json!("done");

    assert!(
        terminal(finished)
            .agent()
            .unwrap()
            .agent_status
            .waits_for_prompt()
    );
}

#[test]
fn a_codex_agent_keeps_its_name() {
    let mut codex = claude_code();
    codex["status"]["app"] = json!("codex");

    assert_eq!(
        terminal(codex).agent().unwrap().agent.as_deref(),
        Some("codex")
    );
}

#[test]
fn the_foreground_leader_names_the_process_group() {
    let info = terminal(claude_code()).process_info();

    assert_eq!(info.pane_id, PaneId("t2q".into()));
    assert_eq!(info.foreground_process_group_id, Some(907_504));
    let [leader] = info.foreground_processes.as_slice() else {
        panic!("one process: {:?}", info.foreground_processes);
    };
    assert_eq!((leader.pid, leader.name.as_str()), (907_504, "claude"));
    assert_eq!(
        leader
            .argv
            .as_deref()
            .and_then(<[String]>::first)
            .map(String::as_str),
        Some("claude")
    );
}

#[test]
fn a_finished_terminal_runs_no_process() {
    let mut finished = claude_code();
    finished["foreground"] = Value::Null;

    let info = terminal(finished).process_info();

    assert_eq!(info.foreground_process_group_id, None);
    assert!(info.foreground_processes.is_empty());
}
