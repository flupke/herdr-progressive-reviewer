# Progressive reviewer

https://github.com/user-attachments/assets/4f0c949d-3eb2-4dd4-bbcc-948ae47b0c41

Main features:

- Per-file turn-based reviews: send feedback to the LLM on a diff range, mark
  file as reviewed, see new diff since your last pass.
- AI-generated inline review guides.
- LSP navigation.
- Full mouse support.
- Syntax highlighting.
- Vim movements.

## Review comments

Select a range with the mouse, or press `V`, move, and press `V` again. An editor
opens below the selection. Click **Post** or press `Ctrl-Enter` to publish the
comment immediately. The editor keeps its text until posting succeeds. Text
still being composed is private to the editor; **Cancel** discards it. `F2`
switches between Vim and regular editing. In Vim editing, `Esc` returns to
normal mode and `i` starts inserting again.

`a` adds a comment at the cursor. Posted messages are immutable; use `A` or
**Reply…** to add a correction or follow-up. Agent replies appear in the same
chronological conversation and can address several comments. Click a message
to select it and use `[c` / `]c` to move between messages. **Resolve thread**
hides the inline thread; its history stays available in Threads, where it can
be reopened. Posted messages cannot be deleted. Threads retain their original
code context when code or files disappear.

Use the **[F]iles | [T]hreads** tabs, `f` to open Files, or `t` to open Threads
(uppercase works too). Threads covers the whole review: `1` selects Unresolved and
`2` selects All in the filter at the bottom of the thread list; `/` opens search
across messages and paths. The search row appears only while editing or applying
a query. All includes resolved history. Each thread has
its own bordered card. Select with arrows, then `Enter` to read;
`A` starts a reply, `r` resolves or reopens, and `p` peeks at the current file.
The full saved diff range is always shown. Peek uses the diff panel’s native
source viewer with syntax highlighting, search, navigation and LSP; `Esc` returns
to the thread. `Alt-u` selects All and jumps to the first unread thread,
including resolved threads.
`Ctrl-t` switches navigation while composing and keeps the unposted text.

Returning to Files preserves the review location. Peeks do not unreview files
or replace a thread's original context. Review threads use the same message
layout as inline diff threads, including syntax-highlighted original code.
Reviewed files hide their code diff while keeping inline conversations and
reply fields accessible. Resolved threads show their summary and Unresolve button. A bottom-right Resolve button collapses a thread
to a separate summary box without enclosing the code, which remains ordinary
file diff; Unresolve restores its conversation, code frame and draft. These buttons
also appear at the bottom right in Threads. Click the underlined filename above
the saved code to open that file in Files; the Peek button is omitted.
While replying, Cancel, Post and Resolve share one row when space permits;
only Post has the green default-action background.
The keyboard shortcuts `r` and `p` remain available.
File badges next to their names show 📄 for guides and 💬 when
unresolved conversations remain, without a counter. A red `●` marks unread
replies beside the name and in the Threads tab title, including late replies
on resolved threads. Directories have no thread or unread badges. File indicators
remain visible after a file is reviewed. Resolution, unread replies and waiting threads
are separate from file progress. The [design and reference views](plan/threads-ui.md)
are saved in `plan/`.
Replies are marked read automatically after every part has appeared in Files or
Threads, including while scrolling through an answer taller than the pane.
Opening a thread alone does not mark it read. Hidden or obscured text stays
unread; seeing a later answer does not clear earlier unseen answers.

An idle reviewer keeps its existing frame. Input and background updates repaint
it; timers also process deferred file loads, guide animation and toast deadlines.
The conversation worker sleeps until an event arrives when no comments need
notification.

Codex and Claude Code read and reply through MCP. Posting wakes the selected
agent through Herdr when it is idle or done. During review work the agent checks
for new comments through MCP, including before finishing. A comment that arrives
after its final read wakes it once it becomes idle. Returning to idle does not
repeat the same notification if MCP is unavailable or the agent stops before
retrieving comments. Post a follow-up to request another attempt. Use the Post
button or `Ctrl-Enter` to post composer text; `Ctrl-s` has no reviewer action.

Automatic notifications wait while the agent pane is focused or its input
contains unfinished text or an image. A notice explains the delay; the comment
is already saved and available through MCP. Notification retries once the
unfocused input is visibly empty. Codex's dim placeholder and animated dots are
recognized from their terminal styling. Unrecognized input layouts also defer the
notification. Herdr currently has no atomic empty-input check with submission,
so a focus change or new input between the check and submission remains possible.

### Connect the agent through MCP

Opening the reviewer automatically registers its MCP server in the project's
`.codex/config.toml` and `.mcp.json`. Existing settings and other servers are
preserved. Setup runs once per project; opening the reviewer again leaves a
matching registration unchanged.

After the first setup, reload MCP configuration or restart/resume your agent, and
check `/mcp` for `herdr_reviewer`. You can also install the configuration before
opening the reviewer by running this from the project you review:

```sh
/path/to/herdr-progressive-reviewer/bin/reviewer-control mcp-install
```

To inspect the configuration or resolve a conflicting registration manually,
the existing helper prints the entries for either client:

```sh
/path/to/herdr-progressive-reviewer/bin/reviewer-control mcp-config codex
# Merge stdout into the project's .codex/config.toml
/path/to/herdr-progressive-reviewer/bin/reviewer-control mcp-config claude
# Merge stdout into the project's .mcp.json
```

Codex reads project configuration in trusted projects. Claude Code may ask you
to approve the project MCP server. Tool calls follow each client's MCP
permissions; allow the reviewer tools when prompted. The agent launches `reviewer-mcp`
and loads its tools even when the reviewer pane is closed. Calls report that the
reviewer is unavailable until it opens; subsequent calls reconnect automatically.
A running agent must load newly installed or migrated configuration once.
In Codex, `/mcp` can show an inventory with `unknown` runtime status even though
the active conversation has not loaded those tools. See the official
[Codex MCP setup](https://learn.chatgpt.com/docs/extend/mcp?surface=cli) and
[Claude Code MCP setup](https://code.claude.com/docs/en/mcp).

The agent-owned stdio bridge forwards calls without changing their arguments
or automatically replaying them. Automatic setup migrates the previous matching
HTTP registration to this bridge, preserving tool policies and disabled settings.
Customized or conflicting registrations remain untouched.

The reviewer hosts Streamable HTTP on `127.0.0.1` only while its pane is open,
using the official Rust MCP SDK, `rmcp`. The default port is stable for the
repository's canonical path. Moving the repository requires refreshing the
configuration. For a port conflict or simultaneous reviewers of the same
checkout, set `HERDR_REVIEWER_MCP_PORT` to a distinct nonzero port in each
reviewer's environment and when printing its client configuration. A busy port
is reported; the reviewer never silently switches addresses.
Automatic setup retains an existing `herdr_reviewer` registration when its URL
differs and reports the conflict. Invalid configuration and symbolic links are
also left unchanged.

When two workspaces use the same checkout, override the second agent's bridge port at
launch so the shared project configuration keeps working for the first:

```sh
# Start this workspace's reviewer with HERDR_REVIEWER_MCP_PORT=59123.
codex -c 'mcp_servers.herdr_reviewer.args=["59123"]'
claude --mcp-config '{"mcpServers":{"herdr_reviewer":{"type":"stdio","command":"/path/to/herdr-progressive-reviewer/bin/reviewer-mcp","args":["59123"]}}}'
```

Conversation updates use a storage lock so reviewers sharing a checkout cannot
overwrite one another's posted messages.

The tools are `list_threads`, `get_thread`, `get_new_messages`, and `reply`.
Every updated thread includes its full conversation and original code context.
The Herdr wakeup supplies a review access value tied to the selected agent
session and logical review. Notifications wait until Herdr reports the selected
agent's session ID, so a late identity report cannot invalidate newly sent access.
Resuming the same native session in the same pane
keeps its access value valid; a different session cannot reuse it. Closing the
reviewer invalidates its values. Reopening with unread comments sends a fresh
wakeup when the selected agent is idle. Already retrieved comments require a
follow-up. Agents can
read and append replies. Thread creation and resolution remain reviewer actions.

This development version starts a fresh conversation store. Legacy comment and
queue files are left untouched. E2E tests use a real isolated Herdr server, a
real MCP HTTP client, and deterministic agent processes with private paths.
The native client test also checks registration after a conversation starts,
MCP reload without restarting that conversation, startup with the reviewer unavailable,
connection while open, and recovery after reopening. It uses isolated Codex and
Claude configuration plus a local model fixture, and makes no model API calls.
Verified with Codex 0.154.0 and Claude Code 2.1.220. Codex keeps the same
process and conversation throughout the lifecycle test; Claude uses fresh
`mcp get` connection checks. The protocol test also keeps one client connected
through closed/open/reopened phases and checks exact reply arguments.

## Language servers

Hover, definitions, type definitions, and references use these executables:

| Files | Server command |
| --- | --- |
| Rust (`.rs`) | `rust-analyzer` |
| Elixir (`.ex`, `.exs`, `.eex`, `.heex`) | `expert --stdio` |
| TypeScript (`.ts`, `.tsx`, `.mts`, `.cts`) and JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`) | `tsgo --lsp --stdio`, falling back to `typescript-language-server --stdio` |

Install [Expert](https://github.com/elixir-lang/expert/blob/main/pages/installation.md)
and [tsgo](https://github.com/microsoft/typescript-go) or
[typescript-language-server with TypeScript](https://github.com/typescript-language-server/typescript-language-server#installing)
separately or provide them through your project's direnv environment. When
`direnv` is available on the reviewer's `PATH`, servers start through
`direnv exec <project-root> <server> ...`. Otherwise, servers use the inherited
`PATH` directly. Direnv approval and setup failures are reported without
falling back to the inherited environment. Startup allows up to five minutes
for direnv and Nix to prepare dependencies. `gR` loads the environment again.
TypeScript server selection happens after loading that environment: `tsgo`
is preferred whenever it is on `PATH`. The fallback is used only when `tsgo`
is absent, not when it fails to start.

The reviewer starts each server when a supported file is opened or queried.
It discovers the repository from the focused Herdr pane's working directory.
Project roots come from `Cargo.toml`, `mix.exs`, or
`tsconfig.json` / `jsconfig.json` / `package.json` in the file's ancestors,
up to the repository root. The outermost matching directory is used so
workspace members share a server. Files without a matching marker use the
repository root.

Mixed-language repositories keep independent servers. `gR` restarts active
servers and reopens their documents. Expert builds and indexes its project
after initialization; navigation results may be empty until that work finishes.

Optional real-server tests run in temporary projects:

```sh
cargo test -p review-lsp --test language_servers -- --ignored
```

## Development install

Requires Rust 1.89 or newer, a C compiler, and Make.
`make check` also uses Herdr, Codex, Claude Code, Python 3, `cargo-nextest`, and
`cccc` for its integration tests and code checks.

Build both programs and link this directory:

```sh
make install
```

The Herdr action list then contains `open`, `close`, and `toggle`.

To diagnose UI stalls, set `HERDR_REVIEWER_TIMINGS` to a JSONL file path when
starting the reviewer. It records event queue delays, handler times, and frame
render times.

## Use

Example configuration, to put in `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+d"
type = "plugin_action"
command = "herdr.progressive-reviewer.toggle"
description = "toggle progressive reviewer"
```
