# Keep a Codex fork read-only with the plugin's guard and its session's settings

Run-ahead forks a Codex agent's session on its daemon (`thread/fork`, then `turn/start`), and
the fork's turn runs inside the daemon, with the agent's MCP tools. A fork must read code and
submit its turn, and nothing else: a tool the user auto-approved can act outside the machine,
such as sending a message, on an answer the reviewer never gave. Claude Code's forks get a
`PreToolUse` guard through `--settings`, which leaves the tool list, and so the prompt cache, as
the agent has them (`claude-fork`). Checked with Codex 0.162.1 on the local fixture: a fork with
the read-only sandbox and an approval policy that asks (`on-request`, `untrusted`) still runs,
without asking, an MCP tool the user set to `approval_mode = "approve"` and a tool its server
marks `readOnlyHint`; Codex's `PreToolUse` fires before every tool call, MCP and nested `exec`
tools included, and a deny stops it.

A fork runs under three guards. The reviewer's Codex plugin registers a `PreToolUse` hook
(`crates/codex-hooks`) that acts only in a session a reviewer marked as a fork's, a file under
`$XDG_RUNTIME_DIR/herdr-reviewer/codex-forks/`: there it allows what Claude Code's guard allows
(`fork-guard`: the two submits and the shell commands that only read) and denies the rest; in
any other session it exits at once, after one file check. Run-ahead refuses to fork when the
daemon lists that hook as not installed, disabled or untrusted. The fork's session gets the
read-only sandbox and `on-request` approvals through `thread/settings/update` before its turn:
Codex appends the permissions after the history, so the fork's requests still start as the
agent's. The reviewer is the fork's only client, so the daemon asks it each approval: it accepts
the two submits and the reading shell commands, and declines everything else
(`crates/codex-fork`). A session keeps its settings once a TUI resumes it, so before the agent
resumes a fork's session, the reviewer lifts the mark and gives the session back the agent's
settings, which the fork's answer gave.

Considered:
- A `config` override of `thread/fork` that removes the other MCP servers: it changes the tool
  list, the first input item, so the fork loses the agent's prompt cache.
- A `config` override that sets every configured MCP tool to ask
  (`mcp_servers.<server>.default_tools_approval_mode = "prompt"`): it leaves the request as the
  agent's, but cannot name a server a plugin brings (the fork fails), and it stays on the
  session after the agent resumes it, where nothing reverts it, so the agent's own tools ask
  from then on.
- Having the agent continue on a second fork of the fork, made with its own settings: a second
  fork per prepared turn, and the agent's next turn under another session.
- The sandbox and the approval policy alone: they let auto-approved and read-only-marked MCP
  tools run.
- A guard hook in every session, not only in marked ones: it would refuse the user's own work.
