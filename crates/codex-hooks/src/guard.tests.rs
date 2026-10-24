use serde_json::json;

use super::*;

#[test]
fn a_fork_reads_and_submits_and_does_nothing_else() {
    for allowed in [
        "mcp__herdr_reviewer__submit_question",
        "mcp__herdr_reviewer__submit_conclusion",
        "view_image",
    ] {
        assert_eq!(refusal(allowed, &json!({})), None, "{allowed}");
    }
    for refused in [
        "apply_patch",
        "create_goal",
        "collaborationspawn_agent",
        "read_mcp_resource",
        // A tool Codex runs without asking: the user approved it, or its server marks it
        // read-only.
        "mcp__slack__send_message",
        "mcp__herdr_reviewer__reply",
    ] {
        assert!(refusal(refused, &json!({})).is_some(), "{refused}");
    }
}

#[test]
fn a_fork_s_shell_command_runs_only_when_it_reads() {
    assert_eq!(
        refusal("Bash", &json!({"command": "rg -n policy src"})),
        None
    );
    let reason = refusal("Bash", &json!({"command": "jj log"})).unwrap();
    assert!(reason.contains("--ignore-working-copy"), "{reason}");
    assert!(refusal("Bash", &json!({"command": "touch x"})).is_some());
}
