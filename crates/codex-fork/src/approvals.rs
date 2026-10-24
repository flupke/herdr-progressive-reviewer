//! The reviewer's answers to what a fork's turn asks its client: the reviewer is the only client
//! of a fork's session, so the daemon asks it every approval and elicitation of the fork. It
//! allows the two submits and the shell commands that only read, as the forks' guard does, and
//! declines everything else.

use fork_guard::{SUBMITS, command_refusal};
use serde_json::{Value, json};

/// The reviewer's answer to the daemon's request `method`, with `params`, made by a fork's turn:
/// the result it replies with, or the error, and what it declined, for run-ahead's log.
pub(crate) struct Answer {
    pub(crate) result: Result<Value, String>,
    pub(crate) declined: Option<String>,
}

impl Answer {
    fn allowed(result: Value) -> Self {
        Self {
            result: Ok(result),
            declined: None,
        }
    }

    fn declined(result: Value, what: String) -> Self {
        Self {
            result: Ok(result),
            declined: Some(what),
        }
    }
}

/// How the reviewer answers the daemon's request `method`, with `params`, of a fork's turn.
pub(crate) fn answer(method: &str, params: &Value) -> Answer {
    match method {
        "mcpServer/elicitation/request" => elicitation(params),
        "item/commandExecution/requestApproval" => {
            let command = params
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match command_refusal(command) {
                None => Answer::allowed(json!({"decision": "accept"})),
                Some(_) => Answer::declined(
                    json!({"decision": "decline"}),
                    format!("the command `{command}`"),
                ),
            }
        }
        "item/fileChange/requestApproval" => {
            Answer::declined(json!({"decision": "decline"}), "a file change".into())
        }
        "item/permissions/requestApproval" => Answer::declined(
            json!({"permissions": {}, "scope": "turn"}),
            "more permissions".into(),
        ),
        "item/tool/requestUserInput" => {
            Answer::declined(json!({"answers": {}}), "a question to the user".into())
        }
        "applyPatchApproval" | "execCommandApproval" => {
            Answer::declined(json!({"decision": "denied"}), method.to_owned())
        }
        _ => Answer {
            result: Err(format!("the reviewer answers no {method} of a fork")),
            declined: Some(method.to_owned()),
        },
    }
}

/// The answer to an MCP server's elicitation: Codex asks one before an MCP tool call, which the
/// reviewer accepts for the two submits alone.
fn elicitation(params: &Value) -> Answer {
    let server = params
        .get("serverName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let tool_call = params
        .pointer("/_meta/codex_approval_kind")
        .and_then(Value::as_str)
        == Some("mcp_tool_call");
    // Codex 0.162.1 names the tool in the message alone: `... to run tool "<tool>"?`.
    let tool = params
        .get("message")
        .and_then(Value::as_str)
        .and_then(|message| message.split_once("run tool \""))
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(tool, _)| format!("mcp__{server}__{tool}"));
    match tool {
        Some(tool) if tool_call && SUBMITS.contains(&tool.as_str()) => {
            Answer::allowed(json!({"action": "accept", "content": {}}))
        }
        tool => Answer::declined(
            json!({"action": "decline", "content": null}),
            tool.unwrap_or_else(|| format!("an elicitation of {server}")),
        ),
    }
}

#[cfg(test)]
#[path = "approvals.tests.rs"]
mod tests;
