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
Delivery failures retain the literal input for Retry. The kickoff prompt carries the scope and first turn identity; the agent submits
its first question directly. The tools advertise complete schemas derived from the
shared submission types; the kickoff carries behavior instructions without schema
examples. The runner formats later wakeups as labeled plain text: turn identity,
one checkpoint, answer ID, question ID/version, the
selected option's full text/ID/outcome and any exact comment. Corrections, deferrals,
conclusion context and previous errors add details only when relevant. Full questions
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
true: visible replies/recaps, evidence, corrections and human inspection remain
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
References from replies, agenda reasons and consequence lenses are
supporting sources. `Question.evidence` requires both `relationship` and
`decision_relevance`; `Question.supporting` accepts background references. Sources
are deduplicated by path, side and range, retaining stable viewer
identities. Every next question requires two to five distinct alternatives.
`Question::choices` adds the built-in None of the above choice, with a stable ID
and open outcome, without rewriting the posted question. Its ID and label are
reserved so agent alternatives cannot duplicate it. Choice selection and text
editing share one answer: Send records both, while Defer and corrections retain
their separate semantics. The question is rendered directly above this form.
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
state and correction link before restoring the destination draft.

Conclusion pages are retained by request identity, in posting order alongside
questions. Each keeps its task editor, reply draft and delivery state. Only the
current conclusion can start implementation. The explicit Implement action sends
only the human-edited task list through the shared prompt queue; the result event
confirms delivery, not completed implementation.


### Durable Explore recovery

`review-explore::ExplorePass` stores the existing domain types and delivery records;
`ExploreViewState` stores portable editor/reading state. `Comparison` omits live
source buffers and diff caches from serialization. Restored evidence lazily reopens
only the selected file through working-copy/history readers, including native diff
rendering. Missing paths, ranges or base revisions are local evidence limitations.

`review-store` uses `explore-v1/<logical-review-hash>/` inside its canonical-checkout
namespace. Each pass and its deduplication state share an atomic JSON record. A
per-review lock protects mutations against the latest revision; archived passes
accept only completion of already recorded dispatch attempts. `index.json` retains
pass order. One `<pass-id>.view.json` record stores the reviewer's editors and reading
position without rewriting domain history. Its save sequence continues across
reopening. Explore assumes one agent and one reviewer per repository, with no window
identities, alternate draft sets, or legacy-format migration. Record bounds are
256 MiB for a pass and 16 MiB for an editor/index; readers reject invalid versions,
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
to an archived pass. Queued work stays paused on restore, and retries cannot alter
its authorized scope. Runtime access, connections and caches are never persisted.
Native identity compares agent/kind/value, allowing a resumed pane; unresolved
original bindings and actual replacement conversations fail closed for continuation.
