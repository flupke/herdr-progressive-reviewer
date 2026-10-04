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
fn a_fork_runs_reading_commands_only() {
    for allowed in [
        "jj diff -r @ --git | head -50",
        "git log --oneline -5",
        "rg -n policy src | cut -c1-80",
        "find . -name '*.rs'",
    ] {
        assert!(call("Bash", allowed).is_none(), "{allowed}");
    }
    for refused in [
        "jj new",
        "git commit -m x",
        "rm -rf x",
        "cat a > b",
        "ls; rm x",
        "ls && rm x",
        "echo $(rm x)",
        "find . -delete",
        "cat x\nrm -rf y",
        "rg --pre ./script x",
        "rg --pre=./script x",
        "git diff --output=patch",
        "git grep -Ovim x",
        "tree -o out",
        "sort -o out x",
        "jj diff --config ui.diff-formatter=sh",
        "jj diff --config=ui.pager=sh",
        "jj diff --tool=x",
        "cargo test",
        "",
    ] {
        assert!(call("Bash", refused).is_some(), "{refused}");
    }
}
