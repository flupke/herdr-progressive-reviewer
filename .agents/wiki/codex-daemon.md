# Codex's app-server daemon

How Codex's daemon, its hooks and its forks behave, what `crates/codex-daemon` relies on, the
reviewer's Codex plugin, and the gotchas of running a Codex TUI in a test. Checked with Codex 0.162.1 (and a daemon that
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
- A session keeps the hooks it was loaded with: a hook trusted, untrusted or installed later
  changes nothing for it until the daemon loads it again. `hooks/list` says what a new session
  would run.
- `UserPromptSubmit` carries the exact text, with no paste wrapper; a bare `/resume <id>` fires
  no hook. Text left in the input box and a typed `/resume <id>` run as one prompt,
  `<text>/resume <id>`, on the session the TUI was on: blocked, the agent stays there. A blocked
  prompt reaches the session's clients as `turn/started`, `hook/completed` with
  `status: "blocked"`, then `turn/completed`, with no `userMessage`.
- MCP servers run in the daemon too, with its environment: the bridge's
  `HERDR_REVIEWER_MCP_PORT` has to be in the daemon's.

## The reviewer's Codex plugin

Why its prompt hook goes by session, to every reviewer of the user, is in
[ADR 0010](../adr/0010-hear-codex-s-prompts-by-session.md), and why its tool hook guards
run-ahead's forks in [ADR 0011](../adr/0011-keep-a-codex-fork-read-only-with-the-plugin-s-guard.md);
`crates/codex-hooks` says how it is installed. What Codex 0.162.1 does with it:

- `codex plugin marketplace add <dir>` records a local marketplace by its path, and refuses one
  of the same name from another path until `codex plugin marketplace remove`. `codex plugin add`
  copies the plugin into `$CODEX_HOME/plugins/cache/<marketplace>/<plugin>/<version>`, again on
  each run, and enables it in `config.toml`; `codex plugin remove` deletes the copy but leaves
  `hooks.state`.
- A plugin's hook gets no option of the user's: `make install` writes the marketplace into
  `bin/codex-plugin` with the checkout's `reviewer-control` in the hook's command, which Codex
  runs as `$SHELL -lc <command>`, with `PLUGIN_ROOT` and `PLUGIN_DATA`.
- `codex plugin add` leaves the hooks untrusted, and an untrusted hook never runs. A trust key,
  such as `progressive-reviewer@herdr-progressive-reviewer:hooks/hooks.json:user_prompt_submit:0:0`
  (`pre_tool_use` for the fork guard), holds no version; its hash changes with the command. `config/batchWrite` with `upsert` on
  `hooks.state` trusts it; no edit removes a key (`null` is refused), so the uninstall writes
  the user layer's `hooks.state` again without it (`replace`, with the layer's version from
  `config/read` and `includeLayers`).
- On stdio, `codex app-server` takes one JSON-RPC message per line, without `jsonrpc`.
- `crates/reviewer/tests/codex_hooks.rs` installs it in a scratch home, as `make install` does,
  and drives a real Codex TUI on an app server started outside its Herdr. It reaches the
  reviewer's MCP server by its URL, without the bridge, and writes the marketplace beside the
  test build's `reviewer-control`.

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
  them running their turn. `turn/interrupt` stops a turn and `thread/delete` removes a thread,
  and ends its running turn too, interrupted; `thread/read` of a deleted thread fails. An interrupted turn leaves its prompt in the history,
  followed by a `<turn_aborted>` developer message.
- `codex exec fork <id> <prompt>` runs one process per fork, as Claude Code's forks do; its
  behaviour is not checked.
- `thread/fork` without overrides answers with the settings the fork inherits, the agent's
  (`approvalPolicy`, `sandbox`, `approvalsReviewer`). `thread/settings/update` changes them for
  the next turns (`sandboxPolicy` takes the shape `sandbox` came in); the next turn appends a
  permissions message after the history, as fork overrides do.
- A session keeps its settings and its `config` overrides once a TUI resumes it: the TUI's
  `/resume` of a loaded session changes neither, nor does a `thread/resume` with `config`.
  `thread/settings/update` has no `config`. A `config` override that names an MCP server the
  configuration lacks, such as a plugin's, fails `thread/fork` (`invalid transport`).
- Approvals go to the session's clients; the one that forked is the fork's only client. Under
  the read-only sandbox, `on-request` and `untrusted` both ask (`mcpServer/elicitation/request`,
  `_meta.codex_approval_kind: "mcp_tool_call"`) before an MCP tool with the default mode, but
  run without asking one set to `approval_mode = "approve"` and one its server marks
  `readOnlyHint`; a tool's own mode wins over `default_tools_approval_mode`. The request names
  the tool only in its message (`Allow the <server> MCP server to run tool "<tool>"?`).
  `{"action": "accept", "content": {}}` runs it; a decline answers the model "user rejected MCP
  tool call". `on-request` runs commands inside the sandbox without asking; `untrusted` asks for
  every one, `ls` included.
- `PreToolUse` fires before every tool a session calls, nested `exec` tools and MCP tools
  included, with these names: `Bash` (with `tool_input.command`), `apply_patch`, `view_image`,
  `get_goal`, `create_goal`, `read_mcp_resource`, `mcp__<server>__<tool>`, and namespaced tools
  joined to their namespace (`clockcurr_time`, `clocksleep`, `collaborationspawn_agent`); `exec`
  itself fires none. Exit 2 with the reason on standard error, or a `permissionDecision` of
  `deny`, stops the call. `create_goal` starts goal continuation turns.
- A fork's `SessionStart` (source `fork`) runs Herdr's Codex hook for the starting pane: Herdr's
  session for the pane stays the agent's (checked in the run-ahead tests).
- Codex asks the model for a title of a session's first prompt, in a request of its own that
  quotes the prompt.

## A Codex TUI in a test

`crates/review-mcp-config/tests/codex_daemon.rs`, `crates/reviewer/tests/codex_hooks.rs` and the
run-ahead tests of `crates/reviewer/src/runtime/run_ahead/codex.tests.rs` run one in a Herdr test
server, on an unmanaged app server and a scratch home, with a local model fixture
(`review_test_support::codex`); they call no model API, and run in `make test`. The fixture
hands each Explore prompt to the test, with the turn's files the prompt names, and calls the
reviewer's tool the test replies with, through Codex's `exec` tool. What they work around:

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
- A session starts its MCP servers when it starts: the TUI starts after the reviewer's MCP
  server listens, or the session has none of its tools.
- Herdr's Codex hook reports a session only with `HERDR_ENV=1`, `HERDR_SOCKET_PATH` and
  `HERDR_PANE_ID` in the daemon's environment, as a daemon started from a pane has them.
