# Explore inline conversation acceptance demo

Subsequent user feedback removed Explore source snapshots and freshness pauses.
These observations describe the earlier build; the current working-copy behavior
and unchanged-code assumption are documented in [usage](usage.md#explore-a-change-experimental).

Observed on 2026-09-17–18 using the native Codex agent, reviewer UI and
rust-analyzer in a disposable Rust repository. The Herdr server, socket,
configuration, state and panes were private. The user authorized the existing
Codex sign-in for this isolated demo; no live user panes were used. The private
server was stopped and the temporary agent home was removed afterwards.

This records the milestone 1 UI correction. The [earlier demo](explore-milestone-1-demo.md)
records the original interview behavior and its separate validation results.

## Two turns in one conversation

The fixture preserved a thread's resolution on reply, included resolved unread
threads in the inbox, and changed a test to assert preserved resolution. It also
contained an unchanged caller. All source and domain context were synthetic.

1. Start showed the frozen checkpoint, unsaved-progress notice, actual waiting
   status and Cancel, without an empty diff frame or answer editor.
2. The agent asked: “When a resolved thread receives a reply, should it stay
   resolved and appear in the inbox while unread?” Its primary evidence opened
   in a fitted native diff window with a yellow relevance outline. Supporting
   references included both policy versions, the unchanged caller, inbox code
   and the changed test.
3. The answer was: “In this toy fixture, resolution records task completion and
   unread records attention. I have not decided when attention should clear.”
   The agent retained that exact context without inferring acceptance.
4. The next question appeared below the first answer and recap: “For a resolved
   thread with a new reply, should viewing the reply clear attention, or should
   explicit acknowledgment be required?” Its evidence and composer were in the
   same scrollable flow. Both questions remained inspectable.
5. Pressing `1` submitted `clear_on_view`, “Viewing the reply clears attention,”
   for `q1` version 2 with outcome `needs_follow_up`. There was no second Accept
   step. The agent recorded a missing view transition and regression sequence
   as work for later; it did not implement them or run the fixture's tests.
6. While waiting for that response, navigation returned to the earlier turn.
   The response did not move its viewport or replace its code state. The closing
   text required further human file inspection in Files.

## Native windows and interaction

- With the first answer still unsubmitted, supporting evidence opened the
  unchanged caller. Native search for `after_reply` and an actual rust-analyzer
  definition request opened `src/policy.rs`. The evidence label retained the
  caller's recorded identity, while the native header identified the displayed
  policy file. Primary returned to the question's evidence.
- Native selection opened an ordinary source-comment editor. Its draft text
  survived keyboard resizing, a mouse drag that changed the window from 24 to
  20 rows, and Fit evidence. The interview answer also survived these actions.
- Earlier turns collapsed and expanded in place, restoring the code view.
  Reflow from 132 to 92 terminal columns wrapped the questions and comment
  draft. Files and Threads retained their existing layouts; returning to
  Explore restored the conversation and native state.
- The header remained **0/3 reviewed** throughout the interview. An external
  edit to the disposable caller then produced **STALE · decisions and LSP
  paused**, retaining the frozen checkpoint and history. Files reflected the
  fourth changed file with **0/4 reviewed**.

## Approved comment-posting follow-up

After explicit authorization on 2026-09-18, the same disposable repository and
private review state were reopened with the installed build. A fresh Explore
pass supplied an inline policy window; interview history was not restored.
The saved comment was recovered after cycling through Files, Threads and
Explore, then posted from the focused native editor:

> Demo-only source comment: explain this line without editing files or running tests.

The persisted thread retained the original draft's message ID, thread ID and
frozen source context. Its saved draft cleared. The real agent retrieved it
through the reviewer MCP and replied with an explanation of the function
signature. The thread contained exactly that comment and its agent reply.
The unsent interview draft survived posting and resizing, the header remained
**0/4 reviewed**, and the fixture's source diff was unchanged. The private server
and temporary agent home were removed afterwards.

## Verification and remaining limits

`make check` passed formatting, complexity, compilation, Clippy, doctests and
**698 tests**, with five optional tests skipped. The separate real rust-analyzer
UI test also passed. Deterministic UI tests cover large and wrapped
evidence, compact unavailable content, separate wheel routing, focus and draft
preservation, delayed responses, error/Retry, stale restrictions, and cancellation
while a capture is still pending.

The native demo exposed a draft shared by multiple evidence windows. Final
regressions verify that posting clears unchanged copies and preserves divergent
text as a saved draft with a fresh publication identity. The approved posting
follow-up also exposed draft recovery being delayed until another thread load.
The final repair restores persisted drafts as primary and supporting evidence
windows open, retaining their publication and frozen-source identities without
writing or posting during initialization. A regression covers changed policy
code and an unchanged caller before any subsequent thread-load event.
Delayed loads preserve edited drafts, and cancellation remains effective when
an older snapshot arrives or another evidence window opens.

The complete manual sequence was not repeated after the final capture,
reply-visibility and draft repairs. The approved posting follow-up preceded the
last draft-recovery repair; that repair has deterministic coverage. The real
LSP test also passed on the final code.

Old/deleted-side LSP and old ranges outside displayed diff hunks retain the
[documented navigation limits](usage.md#explore-a-change-experimental).
Interview progress remains in memory and is lost on close. The interview and
its topic associations do not complete file review; human inspection in Files
is still required. Persistence and later milestones remain outside this update.
