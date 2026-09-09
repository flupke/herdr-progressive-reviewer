# Agent connection through MCP

For the review workflow, see [Using the reviewer](usage.md).

## Architecture

The reviewer uses two separate connections, simplified here:

```text
Agent reads comments or posts a reply:
Codex / Claude --stdio--> reviewer-mcp --localhost HTTP--> reviewer
                                                            |
                                                     conversation store

Reviewer wakes the agent after a comment is posted:
reviewer --PromptGate--> Herdr's Unix socket --prompt--> agent's terminal
```

`reviewer-mcp` is launched by the agent using its MCP configuration. It advertises
the four tools even when no reviewer is running; actual calls connect to the
repository's reviewer. It holds no history. The reviewer owns the HTTP server,
validates access, and reads or updates the conversation store. Closing the
reviewer leaves the bridge alive; a later tool call reconnects after reopening.

`PromptGate` checks that the agent pane is unfocused and its input appears empty
before sending a notification through Herdr. It is separate from MCP access and
approval rules. Its remaining race is described under [Notifications](#notifications).

User-level registration lets both clients find the bridge in every repository. It does
not grant access to every review: each call needs a token for the selected review
and native agent session. Client approval of a tool call is a separate decision;
the optional [reply-preapproval setting](#codex-reply-approvals) changes that client policy.

## Installation and connection

`make install` registers the stdio bridge once in both clients' user configuration:

| Client | Configuration |
| --- | --- |
| Codex | `$CODEX_HOME/config.toml`, defaulting to `~/.codex/config.toml` |
| Claude Code | `$CLAUDE_CONFIG_DIR/.claude.json`, defaulting to `~/.claude.json` |

Clients can load the tools before any reviewer opens, without project configuration.
Reviewer startup and agent notifications do not install or rewrite MCP settings.
The bridge finds the Git or jj repository from the agent's working directory,
including when it starts in a repository subdirectory. It honors
`HERDR_REVIEWER_MCP_PORT` when inherited by the agent. Tool discovery also works
outside a repository; calls then report that repository discovery failed.

To register separately, from any directory (optionally select just one client):

```sh
/path/to/herdr-progressive-reviewer/bin/reviewer-control mcp-install
/path/to/herdr-progressive-reviewer/bin/reviewer-control mcp-install codex
/path/to/herdr-progressive-reviewer/bin/reviewer-control mcp-install claude
```

Repeated installation leaves matching registrations unchanged. Existing settings,
tool policies and other servers are preserved. Conflicting registrations, invalid
configuration and symlinked configuration files are left untouched and reported.
Configuration directories may be symlinks, such as a `CODEX_HOME` in a dotfiles
checkout. The installer does not migrate HTTP or fixed-port registrations.

Older versions wrote project `.codex/config.toml` and `.mcp.json` entries. These
remain untouched and can override user settings. To use automatic repository
routing, remove only their `herdr_reviewer` entries once user registration is
installed; keep any deliberate project overrides and unrelated settings.

An existing Codex conversation started before user-level installation needs
one restart/resume to load the bridge. Subsequent projects do not need this
step. Integrations using Codex's app-server can instead send
`config/mcpServer/reload`, which refreshes loaded conversations.
If a running agent has not loaded the reviewer tools, reload its MCP
configuration or restart/resume it, then check `/mcp` for `herdr_reviewer`.
Tool calls follow each client's MCP permissions; allow the reviewer tools when
prompted. The agent launches `reviewer-mcp`
and loads its tools even when the reviewer pane is closed. Calls report that the
reviewer is unavailable until it opens; subsequent calls reconnect automatically.
A running agent must load newly installed configuration once.
In Codex, `/mcp` can show an inventory with `unknown` runtime status even though
the active conversation has not loaded those tools. Listing the inventory does
not reload the conversation's tool catalog; Codex 0.154.0 has no `/mcp reload`
command. Registering the bridge before startup avoids this mismatch. See the official
[Codex MCP setup](https://learn.chatgpt.com/docs/extend/mcp?surface=cli) and
[Claude Code MCP setup](https://code.claude.com/docs/en/mcp).

The agent-owned stdio bridge forwards calls without changing their arguments
or automatically replaying them.

## Codex reply approvals

The installed Herdr MCP bridge connects over loopback and appends replies to
local review storage. If Codex's automatic approval review rejects
`herdr_reviewer.reply` as sending data to an unverified destination, you can
opt in to preapproving that tool in `$CODEX_HOME/config.toml` (normally
`~/.codex/config.toml`):

```toml
[mcp_servers.herdr_reviewer.tools.reply]
approval_mode = "approve"
```

This permits future reply calls without the normal approval prompt across
projects using that user configuration. Other tools keep their existing
approval settings, and filesystem and network sandbox permissions stay the same.
Managed policies can still restrict approval settings. See Codex's
[MCP configuration](https://learn.chatgpt.com/docs/extend/mcp#other-configuration-options).

For an already-denied reply, run `/approve` in the affected Codex conversation
and select that action to authorize one retry. The retry still goes through
automatic review with your explicit approval as context. See
[Auto-review denials](https://learn.chatgpt.com/docs/sandboxing/auto-review#denials-and-failure-behavior).

## Notifications

Codex and Claude Code read and reply through MCP. Posting wakes the active
agent through Herdr when it is idle or done. During review work the agent checks
for new comments through MCP, including before finishing. A comment that arrives
after its final read wakes it once it becomes idle. Returning to idle does not
repeat the same notification if MCP is unavailable or the agent stops before
answering comments. Use **Retry agent** on an unresolved thread to request
another attempt for unanswered comments without adding a comment; a follow-up also
requests work. Both use the same active agent as filename insertion. Use the Post
button or `Ctrl-Enter` to post composer text; `Ctrl-s` has no reviewer action.

Automatic notifications wait while the agent pane is focused or its input
contains unfinished text or an image. A notice explains the delay; the comment
is already saved and available through MCP. Notification retries once the
unfocused input is visibly empty. Codex's dim placeholder and animated dots are
recognized from their terminal styling. Unrecognized input layouts also defer the
notification. Herdr currently has no atomic empty-input check with submission,
so a focus change or new input between the check and submission remains possible.

## Multiple reviewers and ports

The reviewer hosts Streamable HTTP on `127.0.0.1` only while its pane is open,
using the official Rust MCP SDK, `rmcp`. The default port is stable for the
repository's canonical path. A new agent started after moving the repository
discovers its new path without changing configuration.
For a port conflict or simultaneous reviewers of the same
checkout, set `HERDR_REVIEWER_MCP_PORT` to a distinct nonzero port in each
reviewer's and corresponding agent's environment. A busy port
is reported; the reviewer never silently switches addresses.

Alternatively, override the second agent's bridge port at launch:

```sh
# Start this workspace's reviewer with HERDR_REVIEWER_MCP_PORT=59123.
codex -c 'mcp_servers.herdr_reviewer.args=["59123"]'
claude --mcp-config '{"mcpServers":{"herdr_reviewer":{"type":"stdio","command":"/path/to/herdr-progressive-reviewer/bin/reviewer-mcp","args":["59123"]}}}'
```

Conversation updates use a storage lock so reviewers sharing a checkout cannot
overwrite one another's posted messages.

## Review access and tools

The tools are `list_threads`, `get_thread`, `get_new_messages`, and `reply`.
Every updated thread includes its full conversation and original code context.
The Herdr wakeup supplies a review access value tied to the selected agent
session and logical review. Notifications wait until Herdr reports the selected
agent's session ID, so a late identity report cannot invalidate newly sent access.
Filenames, guide generation and comment delivery share one active-agent selection:
the most recently focused live agent in this workspace, with the existing single-agent
fallback. Focus changes, posts, retries and reopening use that selection; reviews do
not store a separate recipient. Pending notifications also follow focus while waiting
for a native session or an empty input. Waking a different agent retires the previous
access value for that review. Completed answers belong to the review, so the active
agent gets only unanswered work, with each pending thread's full conversation as context.

Access values are bearer tokens. Each request checks that the bound pane still has
the same native session; a session change invalidates that grant. The server does
not authenticate the calling process: another local caller possessing a valid token
can use it. Closing the reviewer invalidates its values. Reopening with pending
comments sends a fresh wakeup when the active agent is idle. Fetched but unanswered comments remain
pending and are recovered as well. Agents can read and append replies. Thread
creation and resolution remain reviewer actions.

Resolved threads are excluded from pending work and future wakeups. An answer already
in flight can still be saved without reopening the thread. Unresolve makes any
remaining unanswered comments eligible again; completed answers are never replayed.

Reading with `get_thread` or `get_new_messages` never consumes work. Each returned
thread includes `in_reply_to`, the last reviewer comment in that snapshot. Pass it
to `reply` along with `message_id` and `text`: saving the answer also acknowledges
comments through that ID. A later comment remains pending even if it arrived
before the answer was saved. Retrying requires the same three values. A rejected
or interrupted reply leaves the comments pending. Existing MCP clients must refresh
their tool definitions after upgrading from the earlier reply schema.

## Runtime and verification

An idle reviewer keeps its existing frame. Input and background updates repaint
it; timers also process deferred file loads, guide animation and toast deadlines.
The conversation worker sleeps until an event arrives when no comments need
notification. Repository and current-file peek refreshes use filesystem events
(inotify on Linux), with no periodic repository scans. The displayed source is watched
even when ignore rules exclude its directory; unrelated ignored output stays excluded.
A watcher failure is reported;
reopen the reviewer to restore live updates.

Conversation-store version 2 is read without dropping history. Retrieval cursors
alone never prove completion. Only unresolved threads still waiting for an answer
are delivered; a final fetch does not make answered threads pending again.
Legacy replies have no exact snapshot boundary, so they cover comments preceding
them in posting order, matching the thread's Waiting status. A follow-up after
that reply remains pending. Replies with `in_reply_to` always use their exact
boundary, including when another comment arrived before the answer was saved.
Version 3 acknowledgement positions are merged across
recipients, preserving completed answers during handoff. The next changed update
migrates version 2 or 3 to version 4 without dropping any history.

Version 4 stores immutable original code in compressed, content-addressed context
files. The conversation index contains messages, read/answer progress and
unposted drafts, with references to that context. Small updates rewrite the index
without recompressing original files, and open views share cached context. Nothing is
pruned, including resolved history. Former recipient fields are ignored when reading
existing indexes. State remains separate for each canonical checkout.
A successful post removes its draft in the same atomic index update. Recovery opens
unposted text for editing; it never publishes or notifies an agent. Cancel and submitting
trimmed-empty text discard the saved draft. Older comment and queue files remain untouched. E2E tests use a real isolated Herdr server, a
real MCP HTTP client, and deterministic agent processes with private paths.
The native client tests check preinstalled user registrations without project
configuration or MCP reload, including forwarding a custom port. They cover
startup with the reviewer unavailable, connection while open, and recovery after
reopening. They use isolated Codex and Claude configuration plus a local model
fixture, and make no model API calls. Runtime tests also verify that opening and
reopening a reviewer leaves project configuration untouched.
Verified with Codex 0.154.0 and Claude Code 2.1.220. Codex keeps the same
process and conversation throughout the lifecycle test; Claude uses fresh
`mcp get` connection checks. The protocol test also keeps one client connected
through closed/open/reopened phases and checks exact reply arguments.
