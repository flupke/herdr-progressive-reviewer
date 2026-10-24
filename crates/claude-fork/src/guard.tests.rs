use super::*;

fn call(tool: &str, command: &str) -> Option<String> {
    refusal(&serde_json::json!({"tool_name": tool, "tool_input": {"command": command}}))
}

#[test]
fn a_fork_reads_and_submits_and_writes_nothing() {
    for allowed in [
        "Read",
        "Grep",
        "Glob",
        "Agent",
        "mcp__herdr_reviewer__submit_question",
    ] {
        assert!(call(allowed, "").is_none(), "{allowed}");
    }
    for refused in [
        "Edit",
        "Write",
        "NotebookEdit",
        "mcp__herdr_reviewer__reply",
    ] {
        assert!(call(refused, "").is_some(), "{refused}");
    }
}

#[test]
fn a_fork_s_shell_command_runs_only_when_it_reads() {
    assert_eq!(call("Bash", "rg -n policy src | head"), None);
    assert!(call("Bash", "rm -rf src").is_some());
}
