# Codex's app-server daemon

How Codex's daemon, its hooks and its forks behave, what `crates/codex-daemon` relies on, and
the gotchas of running a Codex TUI in a test. Checked with Codex 0.162.1 (and a daemon that
updated itself to 0.164.0) and Herdr 0.9.5. Why the reviewer hears Codex this way is in
[ADR 0009](../adr/0009-hear-codex-through-its-app-server-daemon.md).

## The daemon and its socket

- By default, each Codex TUI runs its sessions ("threads") on a daemon shared by every TUI of
  its Codex home. Its control socket is `$CODEX_HOME/app-server-control/app-server-control.sock`,
  a link to `/tmp/codex-daemon-$UID/<digest>`. It carries JSON-RPC in WebSocket text frames over
  the Unix socket, with no authentication beyond the file's mode; `codex app-server proxy` only
  relays bytes. The handshake is `initialize`, then `initialized`, as on stdio.
- The protocol's types: `codex app-server generate-json-schema --out <dir> --experimental`.
- A TUI on `--remote unix://` uses the default socket and `--remote unix://<path>` another; such
  a TUI never starts a daemon. A TUI on `--no-daemon` runs its own app server: no socket, and
  no other client sees its sessions.
- A managed daemon (the one a plain TUI starts) runs a `daemon-updater` that downloads about
  850 MB into `$CODEX_HOME/packages` and restarts the daemon on a new release. Stopping it takes
  `codex app-server daemon stop`, and its `pid-update-loop` process has to be killed as well.
  An unmanaged `codex app-server --listen unix://` binds the same default socket, which a plain
  TUI takes for its daemon, and never updates itself.
- After the daemon restarts, a TUI shows "Reconnecting to server…" and reconnects within a few
  seconds, rejoining its session; a client has to connect and join again by itself.
- The sessions of a TUI on `--remote` run in the server's working directory, not the pane's.

## What a client hears

- Every connection gets `thread/started` (a fork carries `forkedFromId`),
  `thread/status/changed` and `thread/closed`, for every session.
- Turn and item events go only to the session's clients: `turn/started`, `item/completed` (a
  `userMessage` with the exact prompt, `mcpToolCall`, `agentMessage`) and `turn/completed`.
  `thread/resume {threadId}` on a loaded session makes the caller one of them, without
  disturbing the TUI; on a session not loaded, it loads it from its rollout first, starting its
  MCP servers. A fresh TUI's session cannot be joined before its first prompt: "no rollout
  found".
- No event names the client that resumed a session. On every join, the session's clients get
  its goal snapshot: `thread/goal/cleared`, or `thread/goal/updated` with no `turnId` (a turn
  that changes the goal sends one with its `turnId`). It follows the answer to the join at once,
  and reaches the client as soon as the TUI took a `/resume`. The TUI's rejoin
  after a restart sends one too. With 0.162.1, a connection that joined nothing got these
  snapshots as well.
- The daemon keeps no map from clients to sessions, and `thread/loaded/list` lists idle
  sessions for minutes after their last client left. A session names itself in each MCP call:
  `_meta.threadId`, `_meta.sessionId` and `x-codex-turn-metadata`.
- A turn another client starts with `turn/start` shows live in the TUI. `thread/inject_items`
  adds history the model sees and the TUI does not show.

## Hooks

- Under the daemon, hooks run in the daemon's process, with the environment of the TUI that
  started it: a second TUI's hooks carried the first pane's `HERDR_PANE_ID`, and Herdr showed the
  second TUI's session on the first pane. Herdr 0.9.4's docs warn of this; 0.9.5's say Codex's
  state comes from screen detection only.
- `SessionStart` with source `resume` (after a `SessionEnd`) fires only when the next turn
  starts, and Herdr's session for the pane changes only then.
- Under `--no-daemon`, hooks run in the TUI with the pane's environment, from the first prompt.
- A scratch home's hooks run once trusted: write each `currentHash` of `hooks/list` as
  `[hooks.state."<key>"] trusted_hash` in its `config.toml`; auto mode refuses
  `--dangerously-bypass-hook-trust`.
- MCP servers run in the daemon too, with its environment: the bridge's
  `HERDR_REVIEWER_MCP_PORT` has to be in the daemon's.

## Forks

- `thread/fork {threadId}` answers in tens of milliseconds, and `turn/start` on the fork runs
  inside the daemon, with no new process. The fork inherits the model, the sandbox and the
  approval policy, starts its own MCP servers, and sees the reviewer's tools. Its hooks fire in
  the daemon: `SessionStart` with source `fork`, `UserPromptSubmit`, `Stop`.
- Its first request repeats the parent's input except the IDs of its first two items (the tool
  list and the base instructions) and `prompt_cache_key`, which is the thread ID; overriding
  `approvalPolicy` or `sandbox` appends a permissions message after the history. Whether the
  real API serves a fork from the parent's cache under another `prompt_cache_key` is not
  measured.
- Forks live in the daemon, not in the reviewer's process group: a reviewer that dies leaves
  them running their turn. `turn/interrupt` stops a turn and `thread/delete` removes a thread.
- `codex exec fork <id> <prompt>` runs one process per fork, as Claude Code's forks do; its
  behaviour is not checked.

## A Codex TUI in a test

`crates/review-mcp-config/tests/codex_daemon.rs` runs one in a Herdr test server, on an
unmanaged app server and a scratch home, with a local model fixture; it calls no model API, and
runs in `make test` with the native client tests. What it works around:

- The TUI on `--remote unix://` never starts a managed daemon, which would download updates,
  even while the test restarts the server.
- A model migration notice at start (`gpt-5.6-terra` to `gpt-6-sol`) drops what is typed
  meanwhile, and Herdr refuses a prompt to an agent it has not detected: the test uses a model
  with no migration and waits for Herdr's first `idle`.
- A daemon restart right after a `/resume`, before the TUI took a turn, left it with "This
  conversation is unavailable … input is paused": the test takes a turn first.
- The server says `app-server control socket listening` under
  `RUST_LOG=codex_app_server_transport=info` once its socket listens.
- Under `TMPDIR`, Codex warns that it refuses to create its helper binaries there, and goes on.
- The npm package runs the native binary under a Node wrapper of the same name: the test signals
  the native one, and Herdr lists both among the pane's processes.
