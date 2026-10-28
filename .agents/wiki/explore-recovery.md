# Explore recovery: saved rounds

The guarantees of saved Explore rounds across a restart: what is never saved, what a crash can lose, and how each prompt's delivery is recorded. The record format and its bounds are documented in `crates/review-store/src/explore.rs`.

## What is saved

- A missing path, range or base revision is a local limit of the evidence, not a reason
  to drop the discussion.
- Runtime access, connections and caches are never saved.
- Explore assumes one agent and one reviewer per round: there are no window identities and
  no alternate sets of drafts. The workspaces of a repository share its rounds, but a round
  goes on only in the workspace that started it: another workspace's reviewer reopens it
  read-only, since its turns name the starting workspace's root.
- The repository watcher also observes the filesystem events of this state, with no
  polling: a domain update refreshes the round, and an editor autosave triggers no reload
  of the history.

## Editor saves and answers

- Keystrokes not yet flushed can be lost when the process dies abruptly. An acknowledged
  domain change or authorization never depends on that flush.
- The Explore component saves its view once each event settles (`EventSettled`, which the
  application publishes), and its save runs ahead of the event's other actions. A turn posted
  on that event records the sequence of a view that already holds its draft, so a restore
  drops the posted draft instead of showing it again.
- An explicit Retry reuses the logical request ID with a new dispatch-attempt ID, so a
  late cancellation cannot fail a newer attempt.

## Delivery records

- `DispatchObserver` records durable outcomes around the shared `agent.prompt` submission.
  Before the external delivery it commits `Attempting`; a result that was lost recovers as
  `Unknown`.
- A confirmed result wins a race with a cancellation, and can be saved to an archived
  round.
- Queued work stays paused on a restore, and a retry cannot alter its authorized scope.
- What a front end shows of an implementation request comes from one rule,
  `ImplementationState::of`: its dispatch state, and whether this process sends it. The
  session hands the result to the pane in its events and to the page in its publication;
  neither front end reads a dispatch state. The pane's events count as sent by this process
  only the request it sends; the page's publication counts every request while it sends one.
- The identity of the native agent session is compared by agent, kind and value, which
  allows a resumed pane. An original binding that cannot be resolved, and an agent session
  that really was replaced, fail closed: the round does not continue on them.
