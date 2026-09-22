# Explore milestone 1 acceptance demo

The delivery checks described below are historical. Current delivery uses
`agent.prompt` directly; see [Notifications](mcp.md#notifications).

Subsequent user feedback removed Explore source snapshots and freshness pauses.
These observations describe the earlier build; the current working-copy behavior
and unchanged-code assumption are documented in [usage](usage.md#explore-a-change-experimental).

This records the original milestone implementation. See the subsequent
[inline conversation demo](explore-inline-ui-demo.md) for the corrected layout
and its current validation results.

Observed on 2026-09-17 with the native Codex agent in one existing conversation,
the actual reviewer UI, and rust-analyzer. The demo used a disposable Git
repository and a private Herdr server, socket, configuration, state and panes.
The user authorized use of the existing Codex sign-in; the temporary agent home
was removed when the private server stopped. No live Herdr panes were used.

## Fixture

The small Rust project contained a policy function, an unchanged caller, an
inbox predicate and a test. The change preserved a thread's resolution when a
reply arrived, displayed resolved-but-unread threads in the inbox, and changed
the reply test to expect preserved resolution. Three files changed; the caller
remained unchanged until the final freshness check.

The consequential choice was whether a late reply should reopen a resolved
thread, leave it resolved but require attention, or remain outside the inbox.
All domain context and source content in the fixture were synthetic.

## Observed interview

1. Start produced a real question: “Should replies preserve resolution, with
   resolved threads appearing in the inbox whenever they are unread?” The code
   pane opened the policy diff automatically with relevant ranges outlined in
   yellow. Evidence included the before/after policy and inbox behavior, the
   unchanged caller and the changed test.
2. While an answer was still unposted, evidence navigation opened the unchanged
   caller. Searching for `after_reply` and invoking the actual rust-analyzer
   definition operation opened `src/policy.rs`. The question and unposted answer
   stayed available; Primary returned to the question's evidence.
3. The reviewer sent: “These are compliance audit threads. Reopening implies
   remediation failed, but late evidence still needs human attention.” The agent
   retained this as context without inferring acceptance. It refined the question
   to distinguish reading from explicit acknowledgment, without asking permission
   to update the map. The refined question opened its primary evidence.
4. Pressing `1` recorded the stable option `preserve-clear-on-read`, “Stay
   resolved; reading clears attention.” There was no second Accept action. The
   recap distinguished policy acceptance from implementation correctness and
   file-review completion.
5. Correct sent: “Keep this policy only if we add a regression test covering
   resolved unread and resolved read inbox visibility.” The agent recorded a
   correction to the original answer and changed the conclusion to follow-up
   required. It collected the regression coverage as work to do afterwards.
6. Expanded history showed the original question and context, the explicit
   choice, and the appended conditional correction. Switching through Files and
   Threads and back preserved history, evidence and composer text. The header
   remained **0/3 reviewed**. The closing text required further human inspection
   in Files; it did not claim the review was complete.
7. Only after the interview, the harness appended a comment to the disposable
   unchanged caller. Explore became **STALE**, retained frozen evidence and
   history, and rejected another Send with “Frozen checkpoint freshness must be
   current.” No new turn mailbox was created. The Files header now reflected
   four changed files, still with none reviewed.

The repository diff before the final external edit contained only the original
three-file fixture change. The agent did not implement the requested test or
other fixes during the interview.

## Repairs exercised during the demo

An early response incorrectly placed a settled status in a pending-topic update.
Validation rejected it without discarding the recorded answer. This exposed a
missing retry diagnostic: retries now carry the rejection reason, and the
checked-in prompt explains that only the attributed interpretation settles a
topic. A fresh pass then completed the explicit-choice and conditional-correction
sequence above. Restarting the reviewer discarded the earlier in-memory pass,
as the visible limitation states.

The installed Codex idle footer also differed from the existing prompt detector's
supported layout. Delivery now recognizes its exact empty-composer layout;
regression checks retain rejection of drafts, attachments and unknown footers.

## Verification and limits

- `make check`: **683 passed**, five optional tests skipped. This includes
  formatting, complexity, compile, Clippy, doctests, domain/UI tests and isolated
  deterministic agent integration tests. Git and jj capture/freshness cases are
  covered without model API access in CI.
- `cargo test -p review-ui real_rust_lsp -- --ignored --nocapture`: **passed**.
  The real rust-analyzer check navigates checkpoint sources and rejects a delayed
  result after freshness becomes unknown. It complements the manual navigation
  above and covers the final implementation.
- The final old-side coordinate, response-identity and frozen-destination guards
  also have deterministic regression coverage; the complete manual interview was
  not repeated after those final repairs.

This is an observation of one meaningful real-agent interview, not a guarantee
that arbitrary free text will be interpreted correctly. Exact answers remain
authoritative and interpretations remain visible and correctable. Explore does
not run requested tests or fixes. Old/deleted-side LSP and old ranges outside
displayed diff hunks have the documented navigation limitations. Progress is not
restored after closing the reviewer. Persistence and later v1 work remain out of
scope; see [usage](usage.md#explore-a-change-experimental).
