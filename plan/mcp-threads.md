# MCP review threads

This records the agreed behavior for the MCP implementation.

## Conversation

Posting publishes a review comment immediately. There is no saved-but-unsent
comment state or separate send action. Text still being composed remains outside
the shared thread. Posted messages are immutable; corrections are follow-ups.

A thread is a chronological conversation containing reviewer comments and agent
replies. Agent replies are independent messages and can address several comments.
The agent can read threads and append replies through MCP. It cannot create
threads, change reviewer messages, or resolve or reopen threads. Resolution
remains a reviewer action.

## Agent interaction

Keep the existing agent-selection behavior. Support Codex and Claude Code through
the same MCP contract and Herdr's agent-aware prompt operation.

Posting to an idle agent sends a wakeup message through Herdr. Posting while the
agent is active sends no extra message: the agent retrieves new comments through
MCP during its work and checks again before finishing. If a comment arrives
between that final check and the agent becoming idle, the reviewer wakes it then.

There is no separate submission queue. Track which reviewer comments the agent
has retrieved. If it retrieves a comment but stops before replying, do not
automatically restart it. The reviewer can explicitly retry or post a follow-up.
Keep the conversation available for that retry.

## New messages

The new-message tool looks across all threads in the logical review. For every
thread with new reviewer comments, return its entire conversation, including
agent replies, together with its code context and stable identity. Do not repeat
unrelated threads merely because another thread changed.

Reading a conversation does not resolve it. An agent reply does not resolve it,
either. Agent replies must not themselves create new work for the same agent.

## Lifetime

Thread access through MCP lasts only while the reviewer is open. Closing the pane
ends that access. Persisted conversations remain available when the reviewer
reopens, but an agent cannot deliver a reply through MCP while the pane is closed.

## Connection

Host Streamable HTTP directly in the reviewer on localhost. There is no adapter
process or private forwarding protocol. Use the official Rust MCP SDK, `rmcp`,
for the protocol, tools, and HTTP transport rather than implementing them locally.
This requires initial registration in Codex and Claude Code, stable addresses
for concurrent reviewers, and validation of client startup and reconnection when
the reviewer is unavailable.

The initial Herdr wakeup identifies the review and instructs the agent to fetch
its updated threads, respond through MCP, and check for more comments before
finishing. Existing agent sessions may need a restart and resume after the
one-time MCP registration. A native session resumed in the same pane keeps its
access value while the reviewer stays open. A different native session cannot
reuse that value. After the reviewer reopens, loading unread comments schedules
a fresh wakeup for the selected agent when idle, with the new access value.
Already retrieved comments do not restart the agent. Transport reconnection
still depends on the client's connection recovery.

Register MCP automatically when the reviewer opens, using the project's
`.codex/config.toml` and `.mcp.json` with a stable configured port. Preserve
other settings and existing registrations; report conflicting URLs rather than
overwriting another workspace's connection. Also provide a setup command that
can run before opening the reviewer. Existing agents can be restarted and
resumed to activate that configuration. Concurrent workspaces using the same
project directory need distinct URLs supplied through agent launch overrides;
project configuration alone cannot distinguish them. Report a port conflict
instead of silently changing the address.

See the official [Codex MCP setup](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)
and [Claude Code MCP setup](https://code.claude.com/docs/en/mcp), and the
[official Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk).

## Development data

Start fresh with the new conversation format. This feature is still in active
development; do not implement migration of old comments, delivery receipts, or
queued submissions. Leave the old stored data untouched and outside the new
conversation store.
