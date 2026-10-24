use serde_json::json;

use super::*;

/// Codex 0.162.1's request to approve the MCP tool `tool` of `server`.
fn tool_approval(server: &str, tool: &str) -> Value {
    json!({
        "threadId": "fork-1",
        "turnId": "turn-1",
        "serverName": server,
        "mode": "form",
        "_meta": {"codex_approval_kind": "mcp_tool_call", "persist": ["session", "always"],
            "tool_description": "", "tool_params": {}, "tool_params_display": []},
        "message": format!("Allow the {server} MCP server to run tool \"{tool}\"?"),
        "requestedSchema": {"type": "object", "properties": {}},
    })
}

fn action(answer: &Answer) -> &str {
    answer.result.as_ref().unwrap()["action"].as_str().unwrap()
}

#[test]
fn a_fork_may_call_the_two_submits_and_no_other_mcp_tool() {
    for tool in ["submit_question", "submit_conclusion"] {
        let answer = answer(
            "mcpServer/elicitation/request",
            &tool_approval("herdr_reviewer", tool),
        );
        assert_eq!(action(&answer), "accept", "{tool}");
        assert_eq!(answer.declined, None);
    }
    for (server, tool) in [
        ("herdr_reviewer", "reply"),
        ("slack", "submit_question"),
        ("slack", "send_message"),
    ] {
        let answer = answer(
            "mcpServer/elicitation/request",
            &tool_approval(server, tool),
        );
        assert_eq!(action(&answer), "decline", "{server} {tool}");
        assert_eq!(answer.declined, Some(format!("mcp__{server}__{tool}")));
    }
}

#[test]
fn an_elicitation_that_is_no_tool_approval_is_declined() {
    let mut form = tool_approval("herdr_reviewer", "submit_question");
    form["_meta"] = json!(null);

    assert_eq!(
        action(&answer("mcpServer/elicitation/request", &form)),
        "decline"
    );
}

#[test]
fn a_fork_runs_a_command_outside_its_sandbox_only_when_it_reads() {
    let decision = |command: &str| {
        answer(
            "item/commandExecution/requestApproval",
            &json!({"threadId": "fork-1", "command": command}),
        )
        .result
        .unwrap()["decision"]
            .clone()
    };

    assert_eq!(decision("rg -n policy src"), "accept");
    assert_eq!(decision("rm -rf src"), "decline");
}

#[test]
fn a_fork_changes_no_file_gets_no_permission_and_asks_the_user_nothing() {
    for (method, refused) in [
        (
            "item/fileChange/requestApproval",
            json!({"decision": "decline"}),
        ),
        (
            "item/permissions/requestApproval",
            json!({"permissions": {}, "scope": "turn"}),
        ),
        ("item/tool/requestUserInput", json!({"answers": {}})),
        ("execCommandApproval", json!({"decision": "denied"})),
    ] {
        let answer = answer(method, &json!({}));
        assert_eq!(answer.result, Ok(refused), "{method}");
        assert!(answer.declined.is_some(), "{method}");
    }
    assert!(answer("item/tool/call", &json!({})).result.is_err());
}
