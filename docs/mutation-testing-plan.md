# Mutation testing plan

## Goal

Use mutation testing to protect important product behavior. Do not optimize for
a 100% mutation score. A useful test must check a public rule or a stable
invariant. It must not check a private implementation detail only to catch one
mutant.

Run small, reviewed campaigns. Use five to ten related missed mutants in each
batch. Stop after three or four unsuccessful attempts to find a useful test.

## Priority

The candidate counts below come from `cargo mutants --workspace --list`. They
show the size of each area, not its test quality.

| Priority | Production file | Candidates | Behavior to protect |
| --- | --- | ---: | --- |
| 1 | `crates/review-state/src/review.rs` | 21 | Review marks, changed files, stale snapshots, and expired baselines |
| 2 | `crates/review-repository/src/excerpt.rs` | 63 | The exact valid diff excerpt sent to an agent |
| 3 | `crates/review-repository/src/diff.rs` | 90 | Diff structure, file kinds, hunk sizes, and line numbers |
| 4 | `crates/review-store/src/lib.rs` | 88 | Stored review marks, atomic writes, safe paths, and permissions |
| 5 | `crates/review-repository/src/repository.rs` | 162 | Repository parsing, snapshot identity, statistics, and stable polls |
| 6 | `crates/review-ui/src/app.rs` | 264 | The main UI state machine and rejection of late results |
| 7 | `crates/review-ui/src/input.rs` | 152 | Review, selection, navigation, and output actions |
| 8 | `crates/review-ui/src/presentation.rs` | 115 | Visible rows, source positions, searches, and excerpts |
| 9 | `crates/review-lsp/src/session.rs` | 95 | LSP initialization, requests, retries, timeouts, and shutdown |

The first interrupted workspace run found 67 missed mutants and one caught
mutant in `review-lsp/src/session.rs`. This is evidence of a large test gap,
but LSP support is after the core review path in the campaign order.

## Campaign order

### 1. Review state

Run all candidates in `review-state/src/review.rs`. Protect these rules:

- A mark applies only to the same review identity.
- An empty interdiff means `Reviewed`.
- A non-empty interdiff means `ChangedSinceReview`.
- A missing baseline removes the old mark and reports `BaselineExpired`.
- An unknown stored schema reports `UnknownSchema` without a fatal error.
- Diff content uses the stored review baseline when one exists.

### 2. Diff parsing and excerpts

Run `excerpt.rs` before `diff.rs`. The excerpt test can use `git apply --check`
as an independent oracle. Protect these rules:

- Selected additions, deletions, and context produce correct hunk ranges.
- Excerpts preserve required file headers and no-newline markers.
- Excerpts reject binary, conflict, and unsupported notices.
- Parsed rows have correct old and new line numbers.
- Invalid hunk sizes produce an unsupported notice.
- Binary files, symbolic links, submodules, and conflicts remain visible as
  notices and do not become text patches.

### 3. Review storage

Run `review-store/src/lib.rs`. Start with record validation and lifecycle.
Protect these rules:

- Repository roots, change IDs, and paths cannot share state by accident.
- UTF-8 and non-UTF-8 paths round-trip without loss.
- Invalid or corrupt records do not become valid review marks.
- An unknown schema is different from an absent or corrupt record.
- Writes are atomic and concurrent readers never see partial JSON.
- Record files and directories keep user-only permissions.
- Removing a missing mark succeeds.

### 4. Repository snapshots

Do not start with all 162 candidates. First select parsing, statistics,
identity, and stable-poll functions. Protect these rules:

- Jujutsu and Git records preserve lossless paths and file kinds.
- Added, changed, deleted, renamed, conflicted, and type-changed files are
  classified correctly.
- Line statistics attach to the correct file.
- Snapshot identity rejects incomplete or mixed command output.
- A poll returns `ChangedDuringPoll` when the identity changes during the
  poll.
- Repository discovery selects the nearest applicable repository.

Test the Git and Jujutsu backend files only after these shared rules pass.

### 5. UI state and selection

Run `app.rs`, then `input.rs`, then `presentation.rs`. Select mutations around
one user flow at a time. Protect these rules:

- Results for an old snapshot or file do not change current state.
- A failed output keeps the selection, and a successful output clears it.
- Review actions keep selection and pending state consistent.
- Cursor movement cannot select headers, notices, or hidden rows.
- Search wraps in both directions and uses the current visible source.
- Expanded gaps and whole-file views keep correct source positions.

Prefer assertions on `Action`, visible state, and rendered output. Do not
assert private call order.

### 6. LSP session

Run `review-lsp/src/session.rs` after the core review campaigns. Use controlled
LSP messages and time values. Do not start a real `rust-analyzer` process for
each mutant unless the behavior cannot be tested at a smaller boundary.
Protect these rules:

- Initialization, quiescence, ready state, query state, retries, shutdown,
  and stopped state have valid transitions.
- Only the matching response ID completes a request.
- Retry and timeout events keep the request identifiers needed by the UI.
- Document versions change only when source content changes.
- Position encoding and returned locations remain inside the Rust workspace.
- Shutdown kills and reaps the child process.

## Commands

Start each campaign with a clean baseline:

```sh
make check
```

Preview one file:

```sh
cargo mutants --workspace --list --diff \
  --file crates/review-state/src/review.rs
```

Run one file with Nextest:

```sh
cargo mutants --workspace --test-workspace=true --test-tool=nextest \
  --file crates/review-state/src/review.rs
```

For a large file, use `--re` to select one related function group. Use
`--iterate` while repairing one batch. Run the same selection without
`--iterate` before the batch is complete.

For each missed mutant:

1. State the product rule that the mutation breaks.
2. Classify the mutant as useful, equivalent, or low value.
3. Add the smallest test at a public or stable boundary.
4. Run `make check` and the focused mutation selection.
5. Review the test before the next batch.

## Stop and defer rules

Stop a batch when its useful mutants are caught, when the remaining mutants
have no stable product oracle, or when the next test would check an
implementation detail. Record a permanent skip only after a person confirms
that the mutant is equivalent or has no useful observable effect.

Defer these areas until the priority campaigns are stable:

- rendering-only widgets;
- themes and syntax colors;
- toast timing and layout;
- thin protocol data types and getters;
- command wrappers, cleanup paths, and process plumbing.

Run the full workspace mutation target only after several focused campaigns.
Use it to select the next important cluster, not as a required score gate.
