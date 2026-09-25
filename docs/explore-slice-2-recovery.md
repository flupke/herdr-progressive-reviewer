# Explore slice 2: durable progress and recovery

Explore saves the investigation under the existing repository state directory in a
versioned `explore-v1` namespace. A pass belongs to the canonical checkout and its
logical review. It retains exact questions, option meanings, answers, corrections,
agenda changes, assessments, conclusions, and source locators. Source contents,
runtime MCP access, native connections, and rendering caches are excluded.

Opening the reviewer restores the newest pass locally. It restores the last saved
page, choices, comments, separate task/reply editors, and reading positions. It sends
no prompt. Previous pass reuses the history controls. Explicit
New pass retains earlier investigations without carrying decisions forward.

## Persistence and acknowledgement

Domain transactions lock and reread the latest pass. Atomic replacement, file sync,
and parent-directory sync complete before acknowledging a post or queuing its
dependent prompt. MCP acceptance commits the response and its deduplication record,
then waits for UI application before returning success. An identical retry cannot
append another entry or navigate twice; conflicting payloads cannot overwrite it.

Explore supports one agent and one reviewer per repository. Each pass has one
`<pass-id>.view.json` editor record, with a sequence that continues across reopening.
The worker coalesces adjacent saves and drains accepted commands on normal close.
Question drafts and conclusion task/reply editors remain independent within that
record; view writes cannot replace conversation history.
An abrupt process death may lose keystrokes whose saves have not completed.
There are no per-window records, alternate saved edits, or legacy-format migration.

Repository filesystem notifications also observe the Explore namespace. Domain
updates refresh pass state; editor autosaves do not trigger history reloads.
There is no periodic state polling. Corrupt, unsupported, missing, or oversized state
produces a local error and preserves its bytes. Intact history remains readable where
possible. Limits are 256 MiB per pass and 16 MiB per view/index;
a growing pass is not limited to the 1 MiB MCP response budget.

## Conversation and delivery boundary

Only the same verified native agent conversation can continue a saved pass. A pane
restart or move is acceptable when that identity matches; a repository path or reused
pane alone is insufficient. Missing/late identity preserves history and edits. An
unresolved original identity or a replacement conversation cannot silently rebind.

The stable pass ID is separate from renewable `review` access. Prompts carry current
access and compact exact answer input. The Explore MCP surface remains only
`submit_question` and `submit_conclusion`; conclusions require the stable `instance`
as well as transport access. There are no source-fetch tools or mailbox fallbacks.

The shared thread/Guide/Explore sender records Attempting inside its delivery gate,
immediately before the external call. A crash from that point recovers as unknown,
even if delivery actually succeeded. Authoritative results can record Delivered,
Not sent, or Cancelled. Superseded attempt callbacks cannot overwrite newer results.

An interrupted interview retains its posted answer. Explicit Retry reuses the
logical turn and exact answer; reopening does not send it. A queued implementation
stays paused. Delivered implementation requests are not repeated. Unknown delivery
asks the reviewer to inspect the original conversation and permits only a deliberate
new request. The immutable authorization contains exactly the task text from its
Implement click; later edits, the summary, and future work cannot expand it.
Delivery does not mean implementation or validation completed.

## Evidence boundary

Reopening does not capture, hash, or scan source for freshness. Evidence is opened
lazily through existing working-copy/history readers. Native source views remain
available when a comparison diff cannot be reopened. Missing files, historical
revisions, or line ranges are local evidence limitations, not reasons to erase the
discussion. The source-unchanged assumption applies across closing and reopening;
use New pass when source has changed. No Explore action marks Files reviewed.

## Validation record

Deterministic tests exercise adaptive-history serialization, corrections and branch
retirement, distinct conclusions and editor ownership, concurrent post/view writes,
long history, invalid storage, interrupted wakeups, renewed MCP access, lost UI
acknowledgements, native identity replacement/resumption, authoritative cancel/send
results, and queued/attempting/delivered implementation recovery. They use the real
private Herdr/MCP infrastructure where transport is involved. UI tests cover local
restoration without posts or file marks, code focus/position/manual height, unavailable
evidence, and draft ownership while a post is being saved.

The fresh real-agent acceptance result and final workflow checks are recorded in the
[slice 2 handoff](explore-slice-2-handoff.md). Earlier first-slice demos are not proof
of this recovery behavior.
