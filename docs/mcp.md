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
reviewer --agent.prompt--> Herdr's Unix socket --> agent's terminal
```

`reviewer-mcp` is launched by the agent using its MCP configuration. It advertises
the six tools even when no reviewer is running; actual calls connect to the
repository's reviewer. It holds no history. The reviewer owns the HTTP server,
validates access, and reads or updates the conversation store. Closing the
reviewer leaves the bridge alive; a later tool call reconnects after reopening.

The reviewer submits notifications through Herdr's `agent.prompt` API. Thread
messages and acknowledgements determine pending work; delivery does not inspect
the agent's terminal layout, focus, or lifecycle state.

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

Codex and Claude Code read and reply through MCP. Posting a comment saves it and
sends the active agent a notification through Herdr's `agent.prompt`, including
while the agent is working. Each new comment requests a notification; changing
agent status or revisiting the review does not repeat an already attempted one.
The agent checks pending threads through MCP, including before finishing. Reading
never consumes comments: a successful reply acknowledges its exact snapshot.

Delivery errors are reported while comments remain saved. Use **Retry agent** on
an unresolved thread to request another attempt without adding a comment; a
follow-up also requests work. Both use the active agent selected for the review.
Use the Post button or `Ctrl-Enter` to post composer text; `Ctrl-s` has no reviewer
action. Filenames and selected diff text are no longer inserted into agent input.

Herdr owns terminal submission and rejects prompts to a blocked agent. The reviewer
does not inspect or protect an existing draft in the agent's composer.

## Repository ports

The reviewer hosts Streamable HTTP on `127.0.0.1` only while its pane is open,
using the official Rust MCP SDK, `rmcp`. The default port is stable for the
repository's canonical path. A new agent started after moving the repository
discovers its new path without changing configuration.
One agent and one reviewer are supported per repository. For a port conflict,
set `HERDR_REVIEWER_MCP_PORT` to a distinct nonzero port in the reviewer's and
corresponding agent's environment. A busy port
is reported; the reviewer never silently switches addresses.

Alternatively, override the agent's bridge port at launch:

```sh
# Start this workspace's reviewer with HERDR_REVIEWER_MCP_PORT=59123.
codex -c 'mcp_servers.herdr_reviewer.args=["59123"]'
claude --mcp-config '{"mcpServers":{"herdr_reviewer":{"type":"stdio","command":"/path/to/herdr-progressive-reviewer/bin/reviewer-mcp","args":["59123"]}}}'
```

Conversation updates use a storage lock to preserve posted messages while delivery
callbacks and agent responses update their records.

## Review access and tools

The comment tools are `list_threads`, `get_thread`, `get_new_messages`, and `reply`.
Every updated thread includes its full conversation and original code context.
The Herdr wakeup supplies a review access value tied to the selected agent
and logical review. When Herdr reports a native session ID, access is bound to
that session. Otherwise it is bound to the agent's foreground process group,
so notifications and **Retry agent** can still reach an agent without a native
session ID.
Explore and comment delivery share one active-agent selection:
the most recently focused live agent in this workspace, with the existing single-agent
fallback. Focus changes, posts, retries and reopening use that selection; reviews do
not store a separate recipient. Waking a different agent retires the previous
access value for that review. Completed answers belong to the review, so the active
agent gets only unanswered work, with each pending thread's full conversation as context.

Access values are bearer tokens. Each request checks that the bound pane still has
the same native session or foreground process group; a change invalidates that
grant. The server does not authenticate the calling process: another local caller possessing a valid token
can use it. Closing the reviewer invalidates its values. Reopening with pending
comments sends a fresh notification to the active agent. Fetched but unanswered comments remain
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

Explore starts with a kickoff prompt containing the repository root, comparison
identity, first request ID, the change description and interview instructions. In
jj the description is the reviewed change's, quoted line by line with `> `; a Git
working tree, or a change without a description, shows `Change description: none`.
The agent inspects the code and calls `submit_question` to post the first question,
about the change's stated purpose, the design decision the rest builds on, or its largest
or riskiest unreviewed area. That first call also carries `design`, the design of the
change in four Markdown parts: `overview` (what the change adds and where), `data_flow`
(its main types and how data flows through them), `algorithm` (the algorithm and its
cost) and `alternatives` (the alternatives the implementer rejected). The tool refuses a
first question without `design`, a part left blank, and `design` on any later turn. A
change that raises no question gets `submit_conclusion` at once, with the design in its
summary.

When the review has earlier rounds, the kickoff then lists the questions the reviewer
decided in them, oldest first, so a fresh reader such as a Challenger does not ask
them again. An answer decided its question when the agent interpreted it as `accepted`
or `needs_follow_up`, or, when the round ended before the agent's turn after it, when it
chose an option with one of these outcomes; that entry gives the option's outcome and
says it was never interpreted. Each entry gives the answer ID with the question ID and
version, the question's text, the chosen option's ID and text, the reviewer's comment
when there is one, the outcome the agent recorded and its follow-ups. The texts are
quoted line by line with `> `, as data rather than instructions:

```text
Earlier decisions of this review, oldest first (text quoted):

Decided answer: answer-id (question policy version 1)
> Keep resolved conversations resolved?
Choice: keep
> Keep them resolved
Comment:
> Include the existing caller.
Outcome: needs_follow_up
Follow-up:
> Cover the existing caller in the change.
```

Answers the agent took up without interpreting them as a decision (context, questions
to the agent), cancelled answers and replies to a conclusion are left out, and so are
rounds the reviewer cannot read, saved by an earlier version. The answer IDs are those
rounds': no interpretation or agenda change of the new round can name them. The first
round of a review has no such section, and later turns of a round do not repeat it.

The tools advertise their full input schemas, including nested questions, evidence,
assessments, agenda changes and interpretations. The kickoff explains the review behavior
without duplicating schema examples.

A round started with a Challenger adds one section to the kickoff and a short reminder to
each wakeup. It is a script for the agent and for the subagent it starts: who proposes the
turn's question, how they exchange facts and positions, and how the single question is
written. The protocol does not change: the agent alone calls the tools, and the challenger
only returns text to it. An agent that can continue a subagent keeps the same challenger
for the round; one that cannot has it keep a handoff file in the system temporary directory.

In such a round, `submit_question` and `submit_conclusion` may carry `challenger_proposals`:
what became, on that turn, of each question the challenger proposed. Each entry has a short
`title`, the same on every turn that reports the proposal, and a `result`: `asked` (it is the
turn's question), `merged` (it was about the same decision as the agent's own question, and
the two became the turn's question), `retired` (a fact settles it) or `kept` (it waits for a
later turn). A `retired` entry also gives `reason`, plain text with the fact that settles it
and the lines that show it:
`{"title": "Retry limit", "result": "retired", "reason": "src/retry.rs new 12-14 already caps
retries at three"}`. The tool refuses an entry without a title, a `retired` entry without a
reason, and any entry in a round without a challenger. The reviewer saves the entries with
the turn, in the round's record, and the statistics command counts them (see the usage
guide). Rounds saved before this field existed load without proposals.

After a human contribution, the shared prompt delivery sends a plain-text wakeup. It
opens with the rules for a later turn (interpreting the answer, marking lines, agenda
changes and concluding), followed, as in the kickoff, by the quiz rules, preparing the
next question and the not-relevant rules, so those rules travel with each answer, then gives:

```text
Explore review access: temporary-access
Explore round: round-id
Explore request: turn-id
Review unit: unit
Checkpoint: commit

Answer ID: answer-id
Question: policy (version 1)
Selected option ID: keep
Selected outcome: accepted

Selected option:
Keep resolved conversations resolved

Comment:
Include the existing caller.
```

The selected option carries its full exact text, stable ID and outcome; the comment
is preserved exactly and omitted when empty. The agent matches question ID/version
to its original question in the same conversation. The checkpoint appears once.
The reviewer keeps the full question and answer for history and validation; its
evidence, assessments, rationale, other choices and recommendation are not resent.

Conditional details appear only when relevant: Previous response error contains the
previous attempt's failure. A `Cancelled answer: <answer ID>` line, before Answer ID,
names each answer the reviewer cancelled since the previous request; the agent
disregards that answer and its own turn after it. The reviewer has already given
back that turn's review marks, and the following answer replaces the cancelled one.
An unselected option is omitted. Replies to a conclusion use Reply to conclusion
with the original conclusion turn ID instead of a question ID/version. The agent
posts the next turn with submit_question directly; no repository catalog or history
dump is sent. get_explore_answer, get_explore and read_explore have been removed.

Inspect source directly from disk and use Git/jj for diffs and historical text.
Git's `checkpoint.review_unit` identifies the base tree; jj's `checkpoint.checkpoint`
identifies the reviewed commit. Include untracked files that Git diff omits; jj merge
bases use the merged parent tree. Cite evidence and topic associations directly using
`{path, side, lines}`: paths are repository-relative UTF-8 strings or raw byte arrays,
side is `old` or `new`, and lines are inclusive and one-based (null for file-level
references). No source registration is needed. Evidence lists the lines the question
is about, most decisive first, and explains what each establishes and how it could
change the answer. Citations never mark lines reviewed.

Every prompt gives the **Unreviewed lines**, the changed lines no review mark covers,
whoever marked the rest (the reviewer in Files, Jev, or the agent after an answer), as
files: an `Unreviewed diffs:` line names a directory holding one diff per changed
path, at that same path, each row prefixed with its `old` (base) and `new` (current)
line number, which are the numbers citations and marks use. The prompt itself lists no
files or lines. When a path is a file for one changed file and a directory for another,
one of the two diffs goes to the top of the directory as
`__herdr_reviewer_displaced_1__`, a `__herdr_reviewer_index__` file there lists it, and
the prompt adds a `Displaced diffs:` line naming that index. Each prompt has a new directory under the system temporary directory, readable
only by the reviewer's user; the reviewer removes it when it prepares the next prompt
or closes. A prompt whose diffs cannot be written, including when the repository cannot
be read, is not sent. After a human answer,
the next
`submit_question` or `submit_conclusion` carries `reviewed`, the changed lines the
answer settled, and `reopened`, reviewed lines it made matter again, each as
`{path, side, lines}` (null lines for a whole file). The kickoff turn follows no
answer and cannot use them. Every turn, the kickoff included, may carry `not_relevant`:
changed lines the agent read that hold no decision for the reviewer, marked reviewed
without a question of their own. Each entry names its lines in the same form and adds
`reason`, one of `removed_code` (removed code whose removal is what the change is for),
`tested_mechanics` (mechanics that a test covers) and `follows_code` (tests, docs and
manifests that follow the code). A `tested_mechanics` entry also names the covering test
in `test`, as `{path, lines}` at the checkpoint; another entry may name one too:
`{"path": "src/parse.rs", "side": "new", "lines": {"first_line": 10, "last_line": 24},
"reason": "tested_mechanics", "test": {"path": "tests/parse.rs", "lines": {"first_line": 5,
"last_line": 30}}}`. The tool refuses a turn with an entry that has no reason, a
`tested_mechanics` entry without a test, or a test whose lines do not exist at the
checkpoint, and the error names the entry. The reviewer lists each reason and test beside
the lines it marked. Rounds saved before reasons existed keep their marks without one.
The reviewer applies a question turn's marks when the human answers that question, so
review progress moves on the human's action, and a conclusion's marks when it is accepted.
Marks apply only while the code is still the round's checkpoint, and what changed is
recorded for the reviewer to see.

Explore displays Markdown `#` sections for Context (`rationale`, with `visual` appended),
Door and Blast radius (`assessments`), and Notes (the selected evidence's `notes`).
Supply bodies without those headings,
starting with a short summary paragraph and adding detail only when useful. Assessment
`details` may be omitted or empty; the summary must still give the decisive reason, backed
by valid evidence or explicit unknowns. All supplied reasoning and unknowns are visible
without an expansion button. Explain unfamiliar implementation concepts in Context, adapting
to the reviewer's demonstrated knowledge, while keeping questions and choices in plain language.

Send each complete structured turn with `submit_question`, using the supplied review
value and the result in `update`. Refresh an already-running agent's MCP tool catalog
after upgrading; Explore exposes `submit_question` and `submit_conclusion`. The old
`submit_explore` name has been removed. `submit_question` requires a next question
and cannot carry a conclusion.
Concept exploration drives the interview: reviewed lines do not exhaust its useful
questions. When no useful inquiry remains, the agent checks the unreviewed lines for
missed concepts, asks further questions only when that reveals one, and then
concludes. With a reviewer-process `TYPESAFE_API_KEY`, Jev marks what it judges
insignificant when a round starts; the prompts do not mention it.
The durable `instance` is distinct from renewable `review` access. Access is never
saved with the round. Reopening rotates it; the next explicit reviewer action supplies
current access through the existing wakeup. Each call checks the pinned native
conversation, then validates against the latest stored round under its lock, atomically
saves the update and deduplication record, publishes it to the UI, and waits for UI
application before acknowledging it. Validation errors leave the request open for repair;
transport retries must reuse the identical semantic payload (with current `review` access after reconnection). A response saved before a lost acknowledgement is restored locally; it is not regenerated. Accepted retries return
`accepted: true, applied: false`. Cancelled, obsolete or changed accepted payloads
are rejected. Explore never writes ordinary thread replies or uses response files.

Use `submit_conclusion` for the separate conclusion screen. Its top-level arguments are:

```json
{
  "review": "temporary-access",
  "instance": "round-id",
  "request": "turn-id",
  "checkpoint": {"review_unit": "unit", "checkpoint": "commit"},
  "interpretation": null,
  "summary": "Review outcome, decisions and remaining uncertainty.",
  "to_be_implemented": "1. First agreed task.\n2. Second agreed task.",
  "future_work": "Optional or later work outside this implementation scope.",
  "quiz": [],
  "quiz_empty_reason": "The change only rewords the summary toast."
}
```

The three sections are separate strings. Use an empty string for no implementation
or future work. The final answer still needs its attributed interpretation when it
records a decision; the same exact-answer and retry rules apply. There are no
question, evidence, reply, topic or agenda fields in a conclusion submission.
`quiz` holds at most three questions at whiteboard level, each `{question, answers,
correct, why, proof, level}`: two to four `answers`, the zero-based index of the
`correct` one, the sentence that says `why`, the `proof` citations of the lines that
establish it, and `level`, the agent's own reason the item is at whiteboard level, which
the round keeps and the page does not show. An empty quiz needs `quiz_empty_reason`; a
quiz with items leaves it null. The tool refuses an item with no correct answer or no
proof lines. The reviewer answers the quiz on the Explore page, which grades each pick
itself; the pane ignores it.
Summary and future work are displayed separately. Only `to_be_implemented` seeds
the editable task box. Submitting a conclusion does not start implementation.
A conclusion saves its outcome before acknowledging success; it can carry `reviewed`
and `reopened` for the final answer like any turn after an answer, and changes no
other review marks. Lines no answer settled stay unreviewed for the reviewer.
The human's **Implement** action sends the edited box contents through the shared
reviewer-to-agent delivery queue, authorizing those tasks and their validation.
It waits for the pinned agent conversation, supports cancelling queued delivery,
and reports delivery failures without discarding edits. Authorization saves the exact
edited scope and logical delivery ID before queuing. The shared dispatcher saves an
attempt marker before the external call and records the authoritative outcome before
reporting success. Reopening never replays pending work. An unfinished attempt is
shown as delivery unknown, not as a cancelled or definitely unsent request.
Delivery confirmation
means the request was sent, not that implementation has finished.

## Runtime and verification

An idle reviewer keeps its existing frame. Input and background updates repaint
it; timers also process deferred file loads and toast deadlines.
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
files. The conversation index contains messages and read/answer progress, with
references to that context. Small updates rewrite the index without recompressing
original files, and open views share cached context. Nothing is pruned, including
resolved history. Former recipient fields are ignored when reading existing indexes.
State remains separate for each canonical checkout.

Drafts are not part of the shared review thread, so they live in a separate
compressed draft file for each logical review (`drafts/<review hash>.json.zst`, version
1), with its own lock and references to the same context files. Saving or discarding
a draft never rewrites the conversation index. A post is written to the index first;
the posted draft is discarded afterwards. Loading drafts always drops any draft
whose message ID is already in the index, so a posted comment never comes back as a
draft, including after a crash between the two writes. Recovery opens unposted text
for editing; it never publishes or notifies an agent. Cancel and submitting
trimmed-empty text discard the saved draft.

Earlier builds kept drafts inside the conversation index. The first load moves them
to the draft file: it writes the draft file, then rewrites the index without them, still
as version 4. An interrupted move leaves the drafts in both files and the next load
repeats it without duplicates. Earlier builds still load the migrated index and show
no drafts; drafts they save go back into the index and are moved again, replacing the
draft file's copy for the same thread. An unreadable draft file is reported but
never hides the threads, and drafts still inside the index stay there. Older comment and queue files remain untouched. E2E tests use a real isolated Herdr server, a
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
