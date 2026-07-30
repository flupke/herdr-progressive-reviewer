# Progressive reviewer MVP implementation specification

Date: 2026-07-30

Status: approved

## Outcome

Build a Rust Herdr v1 plugin that reviews the current jj change. It has a
Ratatui file list and unified diff, keeps per-file review baselines across
sessions, and inserts a selected valid diff excerpt into the last focused
agent pane without submission.

The plugin supports pure jj and colocated jj and Git workspaces through one
jj-only path. It rejects Git-only repositories.

## Required contracts

These documents are normative:

- [Herdr plugin integration](../research/herdr-plugin-integration-contract.md)
- [jj review engine](../research/jj-review-engine.md)
- [Concurrent state storage](../research/concurrent-review-state-storage.md)
- [Ratatui flow](../research/ratatui-review-flow.md)
- [Diff range representation](../research/diff-range-representation.md)
- [jj baseline research](../research/jj-review-baselines.md)

If two documents conflict, this specification and the four first contract
documents in the list take priority.

## Architecture

Use one Rust workspace with these packages or modules:

```text
pr-core
  repository identity, jj argv protocol, diff parser, state schema,
  atomic storage, excerpt builder, application state machine
pr-app
  Ratatui process, poll worker, Herdr socket client
pr-control
  open, close, and toggle action process
```

Keep all subprocess and socket I/O behind traits. Core tests use fixtures and
fake clocks. The UI render function does no I/O.

The run-time data flow is:

```text
2 s timer -> jj status -> exact snapshot -> parsed files/diffs -> UI model
Space -> exact jj identity -> atomic path record -> UI model
Enter -> excerpt builder -> resolve same-workspace agent -> pane.send_text
Herdr events -> last focused same-workspace agent
```

Only a complete poll replaces the current UI model.

## Functional requirements

1. `open`, `close`, and `toggle` are explicit plugin actions. Install and
   startup do not open the pane.
2. One review pane can exist in each Herdr workspace.
3. The plugin runs normal `jj status` every two seconds.
4. It uses the full stable change ID to select stored state.
5. It lists the current change files and shows a colored unified diff.
6. File states are unreviewed, reviewed, and changed since review.
7. A review mark stores the full current jj commit ID.
8. Later edits use path-filtered `jj interdiff`.
9. A missing baseline deletes that path record and resets it to unreviewed.
10. Review records use one atomic file for each repository, change, and path.
11. Concurrent panes share last-write-wins state.
12. `Tab`, Vim movement, `v`, `Enter`, and `Space` behave as the UI contract
    specifies.
13. `Enter` inserts one minimal valid unified diff excerpt without a submit
    key.
14. The target is the last focused live agent in the same Herdr workspace.
15. No-agent, binary, conflict, timeout, parse, and small-terminal cases have
    visible non-crashing behavior.
16. The plugin does not edit `.jj` and does not create bookmarks or refs.

## Non-functional requirements

- Rust errors contain operation and path context but no repository file
  content.
- Child processes have a five-second timeout and are reaped.
- State directories and files are user-only.
- Paths remain lossless on Unix.
- Parsed input is bounded. Reject a diff line above 16 MiB and a command output
  above 256 MiB with a visible notice.
- The UI stays responsive while commands run.
- Unknown Herdr response fields do not cause an error.
- The minimum Herdr and jj versions are explicit in the release notes.

## Acceptance checks

The release gate is:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Integration tests create temporary pure and colocated jj repositories and
verify:

- add, modify, delete, rename, binary, conflict, and empty changes;
- a review mark followed by an edit;
- a rebase without false parent changes;
- a missing baseline reset;
- a switch to another stable change and return;
- two concurrent writers for one and for different paths;
- a killed atomic writer;
- open, repeated open, close, and toggle;
- same-workspace agent selection and cross-workspace rejection;
- insertion without Enter;
- 120x30, 60x10, 40x6, and too-small terminal rendering.

Before release, run one manual test with each supported agent TUI. Confirm that
`pane.send_text` inserts the excerpt without submission, and that the user can
add a comment after it.

## Implementation ticket sequence

### I1. Scaffold and lock external contracts

Create the Rust workspace, manifest, error types, command runner, Herdr client
traits, fixture helpers, and version checks.

Done when Herdr accepts the linked manifest and the workspace test gate passes.

### I2. Implement the jj snapshot and diff parser

Implement repository detection, the two-second snapshot transaction, stable
change switching, file records, full diff parsing, and special notices.

Depends on I1. Done when pure and colocated fixture tests cover all file kinds.

### I3. Implement shared review storage

Implement keys, schema, safe path encoding, atomic replace, unreview, stale
record handling, and concurrent tests.

Depends on I1. It can run in parallel with I2.

### I4. Implement baselines and interdiff state

Connect I2 and I3. Implement mark, reviewed, changed-since-review, missing
baseline, rebase, deletion, and rename behavior.

Depends on I2 and I3.

### I5. Implement the Ratatui state machine

Implement the two-pane and narrow layouts, focus, navigation, selection,
markers, notices, and async result messages with `TestBackend` snapshots.

Depends on I2 data types. It can start in parallel with I3.

### I6. Implement valid excerpt generation

Build excerpts from parsed rows, recompute hunk ranges, validate fixtures, and
keep selection on an insertion error.

Depends on I2 and I5.

### I7. Implement Herdr actions and agent insertion

Implement pane ownership, idempotent action behavior, focus tracking,
same-workspace target checks, and `pane.send_text`.

Depends on I1 and I6.

### I8. Integrate, harden, and package

Connect the poll loop, UI, state, and actions. Add limits, timeouts, terminal
restore, manual agent tests, install documentation, and release artifacts.

Depends on I4, I5, and I7.

I2 and I3 are the first parallel branch. I3 and the early UI work in I5 are
the second parallel opportunity. All other dependencies are strict.

## Approved integration decision

Use `pane.send_text` to insert the diff excerpt without submission. The user
adds a comment after the excerpt and submits the complete prompt. Do not use
`agent.prompt`, because it submits immediately.

The minimum Herdr version is 0.7.5.
