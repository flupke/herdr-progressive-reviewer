# MCP bridge

How an agent reaches the reviewer: the stdio bridge and the reviewer's HTTP server, how the bridge is registered, how a repository gets its port, and what the tests verify.

## Two connections

```text
Agent reads comments or posts a reply:
Codex / Claude --stdio--> reviewer-mcp --localhost HTTP--> reviewer
                                                            |
                                                     conversation store

Reviewer wakes the agent after a comment is posted:
reviewer --agent.prompt--> Herdr's Unix socket --> agent's terminal
```

- The agent launches `reviewer-mcp` from its MCP configuration. The bridge advertises the
  six tools even when no reviewer runs: the comment tools `list_threads`, `get_thread`,
  `get_new_messages` and `reply` ([Review threads](threads.md)), and the Explore tools
  `submit_question` and `submit_conclusion` ([Explore agent contract](explore-agent-contract.md)).
- The bridge holds no history, and forwards calls without changing their arguments or
  replaying them. A call made while the reviewer is closed reports that the reviewer is
  unavailable; a later call reconnects once it is open.
- The reviewer owns the HTTP server, validates access, and reads or updates the
  conversation store. It hosts Streamable HTTP on `127.0.0.1` only while its pane is open,
  with the official Rust MCP SDK, `rmcp`.
- The reviewer wakes the agent through Herdr's `agent.prompt`
  ([Review threads](threads.md), "Delivery").
- Where the tool is hosted is decided in
  [ADR 0002](../adr/0002-host-mcp-in-the-reviewer.md).

## Registration

- `make install` registers the stdio bridge once in the user configuration of both clients:
  Codex in `$CODEX_HOME/config.toml` (default `~/.codex/config.toml`), Claude Code in
  `$CLAUDE_CONFIG_DIR/.claude.json` (default `~/.claude.json`).
  `reviewer-control mcp-install [codex|claude]` does the same from any directory.
- User-level registration lets a client find the bridge in every repository. It grants no
  access to a review: each call needs an access value for the selected review and native
  agent session. A client's approval of a tool call is a separate decision of that client.
- A repeated installation leaves a matching registration unchanged, and keeps existing
  settings, tool policies and other servers. A conflicting registration, an invalid
  configuration and a symlinked configuration file are left untouched and reported. A
  configuration directory may be a symlink.
- The installer does not migrate HTTP or fixed-port registrations, nor the project entries
  (`.codex/config.toml`, `.mcp.json`) that earlier versions wrote; those can override the
  user settings.
- Starting the reviewer, and waking an agent, never install or rewrite MCP settings.
- A client can load the tools before any reviewer opens, with no project configuration. An
  agent that was running before the registration must load the new configuration once: a
  Codex agent session needs one restart or resume, since Codex 0.154.0 has no `/mcp reload`,
  and its `/mcp` inventory can list the tools with an `unknown` status while the
  session has not loaded them. An integration on Codex's app-server can send
  `config/mcpServer/reload` instead.
- Codex's automatic approval review can reject `herdr_reviewer.reply` as sending data to
  an unverified destination. The user can preapprove that one tool with
  `approval_mode = "approve"` under `[mcp_servers.herdr_reviewer.tools.reply]` in Codex's
  `config.toml`, or authorize one retry of a denied reply with `/approve`, which still
  goes through the automatic review. The preapproval covers every project that uses that
  user configuration; other tools, and the filesystem and network sandbox, keep their
  settings, and a managed policy can still restrict it.
- After an upgrade of the reviewer that changes a tool's schema, a running agent must
  refresh its MCP tool definitions.

## Which reviewer a call reaches

- The bridge finds the Git or jj repository from the agent's working directory, also from a
  subdirectory. Tool discovery works outside a repository; calls there report that
  repository discovery failed.
- The default port is stable for the repository's canonical path, so an agent started after
  the repository moved finds its new path with no change of configuration.
- One agent and one reviewer are supported per repository. For a port conflict, set
  `HERDR_REVIEWER_MCP_PORT` to the same nonzero port in the reviewer's and the agent's
  environment, or give the bridge the port as its argument
  (`mcp_servers.herdr_reviewer.args=["59123"]` for Codex, `"args":["59123"]` in a
  `--mcp-config` for Claude Code). A busy port is reported; the reviewer never switches
  address silently.
- The MCP endpoint is never served on the network.

## What the tests verify

- The e2e tests use a real isolated Herdr server, a real MCP HTTP client, and deterministic
  agent processes with private paths.
- The native client tests check preinstalled user registrations with no project
  configuration and no MCP reload, including a forwarded custom port. They cover a start
  with the reviewer unavailable, a connection while it is open, and recovery after it
  reopens. They use isolated Codex and Claude configuration and a local model fixture, and
  call no model API.
- Runtime tests check that opening and reopening a reviewer leaves the project
  configuration untouched.
- Verified with Codex 0.154.0 and Claude Code 2.1.220. Codex keeps the same process and
  agent session throughout the lifecycle test; Claude uses fresh `mcp get` connection checks.
  The protocol test keeps one client connected through the closed, open and reopened phases
  and checks the exact reply arguments.
