# Explore recovery: saved rounds

The guarantees of saved Explore rounds across a restart: what is never saved, what a crash can lose, and how each prompt's delivery is recorded. The record format and its bounds are documented in `crates/review-store/src/explore.rs`.

## What is saved

- A missing path, range or base revision is a local limit of the evidence, not a reason
  to drop the discussion.
- Runtime access, connections and caches are never saved.
- Explore assumes one agent and one reviewer per repository: there are no window
  identities, no alternate sets of drafts, and no migration of earlier formats.
- The repository watcher also observes the filesystem events of this state, with no
  polling: a domain update refreshes the round, and an editor autosave triggers no reload
  of the history.

## Editor saves and answers

- Keystrokes not yet flushed can be lost when the process dies abruptly. An acknowledged
  domain change or authorization never depends on that flush.
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
