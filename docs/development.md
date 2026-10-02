# Development

See the [README](../README.md#install) for building and installing the plugin.
Repository conventions and the feature workflow are in [AGENTS.md](../AGENTS.md).

## Checks

```sh
make check
```

In addition to the build dependencies, the checks use Herdr, Codex, Claude Code,
Python 3, `cargo-nextest`, `cccc`, and `jq`. Herdr integration tests run private
servers with isolated configuration, state and agent paths.

The opt-in [`jev-evals` suite](jev-evals.md) compares Jev hunk-splitting strategies
against frozen line-level labels. Offline checks and paid live runs are separate;
neither runs during `make check`. The [history-based study](jev-history-study.md)
jointly compares prompts, metadata, exclusion rules and token windows on an
audited corpus from repository commits.

Optional real-language-server tests run in temporary projects:

```sh
cargo test -p review-lsp --test language_servers -- --ignored
cargo test -p review-ui real_rust_lsp -- --ignored --nocapture
```

Explore's domain, Git/jj comparison, durable inputs and isolated selected-agent MCP
tests use deterministic responses; CI does not call a model API. Its optional UI
test uses real rust-analyzer to navigate working-copy sources and reject a
delayed result for another evidence window. Unchanged sources are resolved on demand without a repository catalog or capture. Explore assumes code stays unchanged during review;
it does not check freshness or suspend decisions after edits. The real-agent acceptance demo
is a separate manual check in a disposable repository and private Herdr server.
See the [slice 2 recovery report](explore-slice-2-recovery.md) for current acceptance evidence. The archived [adaptive demo](explore-adaptive-demo.md) predates durable resumption.

See [language server setup](language-servers.md) for the server commands, and
[mutation testing](llm-mutation-testing.md) for mutation-test guidance.

## Terminal UI exploration

The Rust harness in [`tests/tui`](../tests/tui) runs the real reviewer in a
`tui-test` PTY. It reuses the Git/jj repository and Herdr fixtures from
`review-test-support`, with private sockets, config, state, workspaces, and MCP
ports. Use LLM explorations to discover cases for the regression suite.

The pinned `tui-test-rs` beta requires Rust 1.90, provided by the Nix shell.
The harness has its own Cargo workspace and lockfile because `tui-test` requires
`unicode-width` 0.2.2 while the existing Ratatui 0.29 adapter pins 0.2.0.
`make vision` builds the reviewer first and supplies `REVIEWER_BIN_PATH` to the
driver. Set `HERDR_BIN_PATH` to use a specific Herdr executable.

### LLM-directed exploration

Start a persistent reviewer session for an agent to observe and operate:

```sh
nix develop --command make vision
# Select Git instead of the default jj fixture:
nix develop --command make vision VISION_ARGS='--repo git'
```

The Rust `reviewer-vision` driver starts a private Herdr server and a temporary
repository containing several changes, including Unicode and a long line. It
prints the actual terminal text and stays alive, accepting one JSON command per
stdin line. Each interaction returns the latest completed screen and frame ID.

An agent whose shell has no persistent stdin runs the driver in the background
with `--commands PIPE`: the driver creates that named pipe, reads commands from
it, and reopens it after each writer, so every command is one `echo`. Responses
go to stdout, so redirect it to a file and read its last line. Redirect
the log with `>|`: under zsh's `noclobber`, `>` refuses an existing log, so the
driver never starts.

```sh
nix develop --command make vision \
  VISION_ARGS="--json --output $DIR/session --commands $DIR/cmd" >| $DIR/out.log &
echo '{"action":"click","text":"notes.md"}' >| $DIR/cmd
tail -n 1 $DIR/out.log
```

Send `stop` when the exploration is done: it closes the live viewer and frees
the private server. A driver left behind stops by itself after 30 minutes
without a command; `--stop-after-idle MINUTES` changes that, and `0` waits
forever. An idle stop answers with `"reason": "idle"`.

```json
{"action":"observe"}
{"action":"press","key":"?"}
{"action":"press","key":"Escape"}
{"action":"type","text":"A comment with café and 日本語"}
{"action":"click","x":5,"y":3}
{"action":"click","text":"notes.md"}
{"action":"resize","cols":70,"rows":20}
{"action":"observe","after":12,"timeout_ms":3000}
{"action":"wait","text":"Jev: marked","timeout_ms":5000}
{"action":"cells","x":0,"y":0,"width":10,"height":1}
{"action":"screenshot"}
{"action":"note","kind":"checked","text":"Help closes with Escape and restores the diff"}
{"action":"jev","path":"src/math.rs","lines":[2]}
{"action":"turn","timeout_ms":15000}
{"action":"reply","turn":1,"tool":"submit_question","arguments":{"update":{"next":{}}}}
{"action":"reopen"}
{"action":"stop"}
```

Coordinates are zero-based terminal cells. `click` with `text` clicks the first
place the screen shows it, so no column needs counting across wide characters. `type` sends bracketed paste, including
newlines, through the application's editor. `observe` can wait up to 30 seconds
for a frame newer than `after`; `unchanged` explicitly reports a wait without a
new frame. An interaction waits up to one second and allows 100 ms for a changed
screen to settle. A changed frame is evidence to inspect, not proof that the
requested action has finished. For asynchronous work, `wait` for the text the
finished screen shows: it returns as soon as the screen shows it, with status
`shown`, or after `timeout_ms` (default 5000, at most 30000) with status
`timeout`. It checks the current screen first, so wait for text the screen
before the command did not show. Waiting on text instead of a fixed delay keeps
a fast, scripted exploration in step with the UI.
`cells` reports the captured cells' styles when focus or selection is conveyed by
color. `screenshot` saves a PNG of the screen, rendered with a bundled JetBrains
Mono, as `screenshots/frame-N.png` and returns its path: use it to judge how the
UI looks (spacing, alignment, theme), and the text frames for everything else. `jev` stands in for the paid Jev classifier, which vision sessions never
reach: it classifies as insignificant every diff hunk of `path` that adds one of
the one-based `lines` (current numbering) or removes one (base numbering), then
presses `rf` and returns the screen with its "Jev: marked" result, or after ten
seconds. Nearby edits share one diff hunk, as they do for Jev. The script it
writes is kept as `jev-script.json` in the session directory. Write that file
yourself before starting an Explore round to have Jev mark at the round's start.

The workspace's first pane is a stand-in implementation agent: a script named
`claude`, which Herdr detects as an agent, swallows every prompt the reviewer
sends. The reviewer itself records each Explore prompt, once its delivery
finished, as a numbered turn (`turns/turn-000001.json` in the session
directory; `HERDR_REVIEWER_VISION_TURNS` names it). This is test tooling: the
reviewer records turns only in a vision session (`HERDR_REVIEWER_VISION`), as
the files hold access values, and a normal reviewer ignores the variable. `turn` waits for the next
turn and returns it: its number, `kind` (`kickoff`, `wakeup` or `implement`),
whether it was `delivered`, the access value and identity a reply needs, the
reviewer's `answer`, the cancelled answers, the `unreviewed` block and the full
prompt `text`. Each call returns the turn after the last one it returned;
`after` asks from another number. `reply` answers a turn by calling an MCP tool
on the reviewer's real endpoint as that agent: it fills in the turn's access and
the `instance`, `request` and `checkpoint` (inside `update` for
`submit_question`) unless `arguments` sets them, and returns the tool result as
`reply` beside the screen. It refuses a `turn` that is not the latest, so a
script cannot answer a prompt the reviewer has replaced. `arguments` that bring
their own `review` value are sent as they are, to check how the reviewer itself
rejects a late or misplaced turn.
`reopen` starts
the reviewer again in the same private workspace and state, at its initial
100×30 size. `stop`, SIGINT, SIGHUP, and SIGTERM clean up the reviewer, private
server, and temporary repository; so does EOF on stdin when the driver reads
commands from it.

When the driver runs inside Herdr, it splits its own pane and runs a live viewer
there: every frame it captures is painted in the viewer the moment it is
published, so a person can watch the agent explore. The viewer splits the
driver's pane the way the reviewer splits the pane it opens from, across the
side that looks longer (beside a pane at least two and a half times as wide as
it is tall, below any other), and takes the share that shows the whole 100×30
session, at most three quarters of the pane. `--viewer right|down|none` picks
the direction or turns the viewer off, and `--viewer-ratio` the viewer's share
of the split. The viewer shows the session at its own size: a narrower pane cuts
rows at its right edge, and a shorter one paints the rows that do not fit over
its last row. Keys typed in the viewer do not reach the session; `q` closes the
viewer and its pane and leaves the session running. The pane closes when the
driver exits, however it exits: the viewer closes its own pane once the stream
ends, and the system ends the stream of a killed driver too. Outside Herdr,
watch the same stream from any terminal with `reviewer-vision --view SOCKET`,
using the `stream` socket that `session.json` names.

Every session gets a new directory under `tests/tui/target/vision/`, printed with
the observations. `--output NEW_DIRECTORY` selects another location and `--json`
returns JSON lines for automated clients. The directory retains:

- `latest.txt`: an atomically replaced text screen with its frame ID, dimensions,
  and cursor metadata, refreshed even while the agent is thinking.
- `frames/`: the most recent 64 distinct text screens. Style and cursor changes
  count as distinct frames even when the characters stay the same.
- `actions.jsonl`: commands, observations, errors, and agent notes. Note kinds are
  `checked`, `finding`, and `untested`; findings should include expected behavior,
  observed behavior, and reproduction steps.
- `recording-*/`: `tui-test`'s textual asciinema recordings, retained across reopen.
- `screenshots/`: the PNGs `screenshot` saved, named after their frame.
- `session.json`: the private repository path, the live `stream` socket and
  session details. An agent can
  edit files in that repository to exercise filesystem-driven updates.

The driver sets `HERDR_REVIEWER_VISION=1` on its child. In that mode the reviewer
marks completed paints with terminal synchronized-update sequences. A filesystem
watcher follows `tui-test`'s recording and replays it through the library's terminal
emulator, publishing text only at those boundaries. This preserves complete frames
even when one paint spans several PTY reads, or several paints share one read.
Unchanged paints are deduplicated. Capture uses filesystem events rather than
periodic screenshot polling. The private child clears `NO_COLOR` so style
inspection includes the UI's normal selection and focus colors.

Give the agent a feature contract and an exploration objective. Let it choose
actions from what it sees, record checks and findings with `note`, and finish with
the untested areas.

### Regression tests from explorations

Populate [`tests/tui/tests`](../tests/tui/tests/README.md) with deterministic E2E
cases derived from recorded LLM explorations. Keep each case focused on a
reproduced finding, with the expected behavior and relevant interaction sequence.
Preserve the evidence needed to understand the case alongside the test; session
artifacts under `target/` are ignored by version control.

Run the harness checks and any added regression cases with:

```sh
nix develop --command make e2e-tui
```

This target also runs during `make check`. It builds the reviewer, checks the
Rust harness, and runs its tests. The current tests cover frame-capture plumbing;
UI scenarios will be added from explorations.

## Diagnose UI stalls

Set `HERDR_REVIEWER_TIMINGS` to a JSONL file path when starting the reviewer.
It records event queue delays, handler times and frame render times.

### Adaptive Explore protocol

The authoritative agent contract is `crates/review-explore-runner/src/interview.md`.
One `InterviewUpdate` binds a direct reply, optional decision interpretation,
agenda operations and one next question to the exact
outstanding request. `Exploration` validates all citations and the
agenda before applying anything. `submit_question` carries these turns;
`submit_conclusion` carries a separate `ConclusionSubmission` with summary, editable
implementation tasks, future work and final-answer interpretation. Both tools route through the
existing reviewer server to a locked store transaction and then the UI; success acknowledges
validation, durable commit and application. Invalid updates remain pending and return their error
to the agent for repair. Cancelled/obsolete requests cannot apply, and an exact
duplicate is acknowledged without replaying it. Replacing an accepted payload is
rejected. Thread comments, Explore turns and guide requests submit directly through Herdr's
`agent.prompt`. Delivery does not parse terminal output or wait for an idle agent,
unfocused pane, or empty composer. Review access binds to the native session when
available, or the foreground process group otherwise. Cancellation removes unsent
requests and a replaced conversation rejects them.
Delivery failures retain the literal input for Retry. The kickoff prompt carries the scope, the change description (jj only; Git has none) and the first turn identity; the agent submits
its first question directly. The tools advertise complete schemas derived from the
shared submission types; the kickoff carries behavior instructions without schema
examples. The runner formats later wakeups as the later-turn rules (`wakeup.md`: interpretation,
review marks, agenda changes, concluding) followed by labeled plain text: turn identity,
one checkpoint, answer ID, question ID/version, the
selected option's full text/ID/outcome and any exact comment, after the Unreviewed
lines block. Conclusion context and previous errors add details only when relevant. Full questions
and answer records stay internal; preparation does not mutate that history. No input-fetch
tool or separately retained runner input is needed. The reviewer retains the immutable history while
the agent keeps its context in the existing conversation. A contribution to a questionless stopping
point carries Reply to conclusion identifying its closing turn;
that context cannot be interpreted as a policy decision. Session discovery/replacement handling
remains independent of repository freshness.

Agenda lifecycle (retire/supersede/reconsider) is separate from decision status.
Topics hold pending wording, order and prerequisite topic IDs; posted versions and
agent operations are retained in conversation history. Reconsideration names the
original decision and a subsequent attributed decision resolves the flag.
Interpretations are optional for informational contributions. The protocol cannot
mechanically establish whether free text is agreement or whether an assessment is
true: visible replies/recaps, evidence, follow-up answers and human inspection remain
necessary. It validates reference integrity, not the agent's semantic reasoning.

`CodeLocation` carries a repository-relative path, old/new side and optional inclusive
line range. Paths use UTF-8 strings or byte arrays for non-UTF-8 names. There are no
source IDs or source-registration calls. Topic associations use the same locations.
The internal comparison retains changed-file context for native viewers without
listing all repository files. Unchanged sources are resolved on demand; historical
files use the original base and new-side files are read directly. Agents inspect
files on disk and use Git/jj for diffs and historical versions. In Git, the review
unit identifies the base tree; in jj, the checkpoint identifies the reviewed commit.
Submissions require the exact request ID, access value and pinned conversation.
Answers arrive through the shared prompt delivery; no handoff files are created.
Question evidence is one ordered list of citations, most decisive first; citations
from replies, agenda reasons and consequence lenses appear after it. Every citation
requires `notes` that explain its relevance. Citations never mark lines reviewed. Citations
are deduplicated by path, side and range, retaining stable viewer
identities. Every next question requires two to five distinct alternatives.
`Question::choices` adds the built-in None of the above choice, with a stable ID
and open outcome, without rewriting the posted question. Its ID and label are
reserved so agent alternatives cannot duplicate it. Choice selection and text
editing share one answer: Send records both. The question is rendered directly above this form.
Additional LSP destinations are also read inside the root. Historical evidence
outside diff hunks uses a native full base-text view and cannot send old coordinates
to the live language server. Evidence sizing and initial positioning use the same
wrapped range, including yellow borders; fitting again recenters the range rather
than its first line.


Explore renders only the selected question, with a pinned history navigation bar.
New accepted questions select the newest page; ordinary input no longer suppresses
advancement. Applying a duplicate does not navigate. Files and Threads retain their
active pane while Explore prepares its next page. Question identity also selects
the composer draft and evidence window; history navigation saves the full editor
state before restoring the destination draft.

Conclusion pages are retained by request identity, in posting order alongside
questions. Each keeps its task editor, reply draft and delivery state. Only the
current conclusion can start implementation. The explicit Implement action sends
only the human-edited task list through the shared prompt queue; the result event
confirms delivery, not completed implementation.


### Durable Explore recovery

`review-explore::ExploreRound` stores the existing domain types and delivery records;
`ExploreViewState` stores portable editor/reading state. `Comparison` omits live
source buffers and diff caches from serialization. Restored evidence lazily reopens
only the selected file through working-copy/history readers, including native diff
rendering. Missing paths, ranges or base revisions are local evidence limitations.

`review-store` uses `explore-v1/<logical-review-hash>/` inside its canonical-checkout
namespace. Each round and its deduplication state share an atomic JSON record. A
per-review lock protects mutations against the latest revision; archived rounds
accept only completion of already recorded dispatch attempts. `index.json` retains
round order. One `<round-id>.view.json` record stores the reviewer's editors and reading
position without rewriting domain history. Its save sequence continues across
reopening. Explore assumes one agent and one reviewer per repository, with no window
identities, alternate draft sets, or legacy-format migration. Record bounds are
256 MiB for a round and 16 MiB for an editor/index; readers reject invalid versions,
structure and oversized data without
replacing it. Writes sync files and parent directories. The existing repository
watcher's lifecycle also observes domain-state filesystem events; no polling or
per-keystroke history serialization was added.

The application queues coalesced editor saves on the worker before dependent
actions. Normal shutdown drains the worker queue. Unflushed keystrokes can be lost
on abrupt death; acknowledged domain changes and authorizations cannot depend on
that queue flush. Posted answers retain their exact original option and comment.
Explicit Retry reuses the logical request ID with a fresh dispatch-attempt ID, so
a late cancellation cannot fail a newer attempt.

`DispatchObserver` records durable outcomes around the shared `agent.prompt`
submission. Before external delivery, it commits `Attempting`; a lost result
recovers as `Unknown`. A confirmed result wins cancellation races and can be saved
to an archived round. Queued work stays paused on restore, and retries cannot alter
its authorized scope. Runtime access, connections and caches are never persisted.
Native identity compares agent/kind/value, allowing a resumed pane; unresolved
original bindings and actual replacement conversations fail closed for continuation.
