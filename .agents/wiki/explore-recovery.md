# Explore recovery: saved rounds

How Explore rounds are saved and restored: what a record holds, the store's layout and bounds, how editor state is saved, and the durable record of each prompt's delivery.

## What is saved

- `review-explore::ExploreRound` stores the domain types and the delivery records.
  `ExploreViewState` stores the editor and reading state, in a portable form.
- `Comparison` leaves live source buffers and diff caches out of what it serializes.
  Restored evidence reopens only the selected file, lazily, through the working-copy and
  history readers, native diff rendering included.
- A missing path, range or base revision is a local limit of the evidence, not a reason
  to drop the discussion.
- Runtime access, connections and caches are never saved.

## The store

- `review-store` uses `explore-v1/<logical-review-hash>/` inside the namespace of the
  canonical checkout.
- Each round shares one atomic JSON record with its deduplication state. `index.json`
  keeps the order of the rounds.
- A lock per review protects each mutation against the latest revision. An archived round
  accepts only the completion of a dispatch attempt that was already recorded.
- One `<round-id>.view.json` record stores the reviewer's editors and reading position,
  without rewriting the domain history. Its save sequence continues across a reopen.
- Explore assumes one agent and one reviewer per repository: there are no window
  identities, no alternate sets of drafts, and no migration of earlier formats.
- A round's record is bounded to 256 MiB, and an editor or index record to 16 MiB.
  Readers reject an invalid version, an invalid structure and oversized data without
  replacing the record.
- A write syncs the file and its parent directory.
- The repository watcher also observes the filesystem events of this state, with no
  polling: a domain update refreshes the round, and an editor autosave triggers no reload
  of the history.

## Editor saves and answers

- The application queues editor saves, coalesced, on the worker before the actions that
  depend on them. A normal shutdown drains the worker's queue.
- Keystrokes not yet flushed can be lost when the process dies abruptly. An acknowledged
  domain change or authorization never depends on that flush.
- A posted answer keeps its exact original option and comment.
- An explicit Retry reuses the logical request ID with a new dispatch-attempt ID, so a
  late cancellation cannot fail a newer attempt.

## Delivery records

- `DispatchObserver` records durable outcomes around the shared `agent.prompt` submission.
  Before the external delivery it commits `Attempting`; a result that was lost recovers as
  `Unknown`.
- A confirmed result wins a race with a cancellation, and can be saved to an archived
  round.
- Queued work stays paused on a restore, and a retry cannot alter its authorized scope.
- The identity of the native agent session is compared by agent, kind and value, which
  allows a resumed pane. An original binding that cannot be resolved, and an agent session
  that really was replaced, fail closed: the round does not continue on them.
