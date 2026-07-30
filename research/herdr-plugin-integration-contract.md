# Herdr plugin integration contract

Date: 2026-07-30

## Decision

The MVP is a Herdr v1 executable plugin. It has one manifest-declared terminal
pane and three manifest-declared actions: `open`, `close`, and `toggle`.
It does not open automatically.

The minimum supported Herdr version is 0.7.5. It is the first released version
that contains the live-agent operations used by this contract. The plugin uses:

- `plugin.pane.open`, `plugin.pane.focus`, and `plugin.pane.close`;
- `session.snapshot`;
- `agent.list`;
- `pane.send_text`;
- the plugin context and state environment variables.

The implementation must verify this version with a 0.7.5 binary before release.

## Manifest

Use this manifest shape. The final package can add build commands, descriptions,
and more platforms after tests pass.

```toml
id = "herdr.progressive-reviewer"
name = "Progressive reviewer"
version = "0.1.0"
min_herdr_version = "0.7.5"
platforms = ["linux", "macos"]

[[actions]]
id = "open"
title = "Open progressive reviewer"
contexts = ["workspace"]
command = ["bin/progressive-reviewer-control", "open"]

[[actions]]
id = "close"
title = "Close progressive reviewer"
contexts = ["workspace"]
command = ["bin/progressive-reviewer-control", "close"]

[[actions]]
id = "toggle"
title = "Toggle progressive reviewer"
contexts = ["workspace"]
command = ["bin/progressive-reviewer-control", "toggle"]

[[panes]]
id = "review"
title = "Progressive review"
command = ["bin/progressive-reviewer"]
placement = "split"
```

Herdr v1 actions and panes are static manifest entries. The plugin cannot
register them at run time.

## Action behavior

The control executable reads `HERDR_PLUGIN_CONTEXT_JSON`. It uses the context
workspace, tab, pane, and current working directory. It must not use a
different active workspace if the user changes focus while the action starts.

`open` does these operations:

1. Find a live review pane owned by this plugin in the context workspace.
2. If one exists, focus it.
3. If none exists, call `plugin.pane.open` with entrypoint `review`, placement
   `split`, the context pane as `target_pane_id`, its current directory as
   `cwd`, and `focus: true`.
4. Return a clear error if the directory is not in a jj workspace.

`close` finds the owned review pane in the context workspace and calls
`plugin.pane.close`. It succeeds without an error if no review pane exists.

`toggle` closes an existing pane. Otherwise, it performs `open`.

There is at most one review pane for each Herdr workspace. Two concurrent
`open` actions can race. After an open, the control process must list the owned
panes again and close the newer duplicate. The earliest live pane wins.

The TUI exits when its terminal receives a normal close signal. It must restore
the terminal before it exits.

## Pane context

Herdr gives the pane process these authoritative values:

- `HERDR_SOCKET_PATH`;
- `HERDR_BIN_PATH`;
- `HERDR_WORKSPACE_ID`;
- `HERDR_TAB_ID`;
- `HERDR_PANE_ID`;
- `HERDR_PLUGIN_ID`;
- `HERDR_PLUGIN_ROOT`;
- `HERDR_PLUGIN_CONFIG_DIR`;
- `HERDR_PLUGIN_STATE_DIR`;
- `HERDR_PLUGIN_ENTRYPOINT_ID`;
- `HERDR_PLUGIN_CONTEXT_JSON`.

The plugin uses `HERDR_PLUGIN_STATE_DIR` for review records. Herdr only gives
the path. It does not manage the files, schema, locking, migration, or cleanup.

Use a real split or zoomed pane. Do not use popup placement. A popup has no pane
ID and does not take part in pane and agent APIs.

## Insert text into an agent chat

Herdr v1 has no chat-draft API. `agent.prompt` submits text. It is not valid for
this feature. `pane.send_text` sends text to a terminal without an Enter key.
This is the only documented method that can insert text without submission.

The TUI keeps `last_agent_pane_id`. It updates the value from Herdr focus and
agent events when an agent pane in the same workspace gains focus. The review
pane does not replace this value. On startup, if no event history is available,
the TUI uses the focused agent in the same workspace from `session.snapshot`.

On `Enter`, the TUI:

1. Builds the minimal valid unified diff excerpt described in the UI
   specification.
2. Resolves `last_agent_pane_id` again with `agent.get`.
3. Confirms that the pane still exists, is still an agent, and is in the same
   `HERDR_WORKSPACE_ID`.
4. Calls `pane.send_text` once with the complete UTF-8 excerpt and no newline.
5. Shows `Inserted into <agent-name>` after success.

The inserted excerpt is a draft. The user adds a comment after the excerpt and
submits the complete prompt.

If no agent chat exists, it makes no API write and shows:

```text
No agent chat is available in this workspace
```

If the last agent pane closed or moved, it clears the target and shows the same
message.

This contract cannot prove that each supported agent has an empty editable
input. Raw terminal text can go to a modal dialog or an alternate screen.
The user starts insertion from the review UI and owns this check. A later Herdr
draft-input API can remove this limit.

## Repository constraints

The action passes only a working directory. The TUI runs `jj root` from that
directory. Success means the workspace is supported.

- A pure jj workspace is supported.
- A colocated jj and Git workspace is supported through the same jj commands.
- A Git-only workspace is unsupported, even if `git status` succeeds.

The plugin never calls Git to build review state. It never edits `.jj`, creates
a bookmark, or creates a ref.

## Error model

Control actions report one short error to Herdr command logs. The TUI shows
recoverable errors in its status line.

| Condition | Result |
| --- | --- |
| Herdr server is unavailable | Action fails; no local process starts. |
| Plugin is disabled or incompatible | Herdr rejects the action. |
| Directory is not a jj workspace | `open` fails with `Not a jj workspace`. |
| Review pane is already open | `open` focuses it. |
| Review pane is absent | `close` succeeds without a change. |
| Agent pane is absent | Insert does nothing and shows a notice. |
| Agent pane changed workspace | Insert does nothing and clears the target. |
| `pane.send_text` fails | Selection stays active and the error is visible. |
| Review pane closes during a poll | The child command is canceled on exit. |

## Acceptance checks

- `herdr plugin link` accepts the manifest.
- `herdr plugin action list` shows all three actions.
- Repeated `open` leaves one review pane in the action workspace.
- `close` is safe when no pane exists.
- `toggle` opens and closes the same workspace pane.
- The pane starts in both pure and colocated jj workspaces.
- The pane rejects a Git-only repository.
- `Enter` puts text in the last focused same-workspace agent input without an
  Enter key.
- `Enter` does not target an agent in another workspace.
- No-agent, closed-agent, and API-error cases keep the selection and show a
  useful message.

## Primary sources

- [Herdr plugin authoring documentation](https://herdr.dev/docs/plugins/)
- [Herdr CLI reference](https://herdr.dev/docs/cli-reference/)
- [Herdr socket API](https://herdr.dev/docs/socket-api/)
- [Herdr agent automation](https://herdr.dev/docs/agent-automation/)
