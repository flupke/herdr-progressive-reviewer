---
title: Herdr progressive reviewer MVP
status: specified
labels:
  - wayfinder:map
---

## Destination

Produce an approved implementation specification and an executable ticket plan for a Herdr plugin that reviews the current jj change.

## Notes

- Domain: Rust, Herdr plugins, jj, Ratatui.
- Supported repositories: pure jj workspaces and colocated jj and Git workspaces. Both use the same jj-only code path. Git-only repositories are unsupported.
- Poll with normal `jj status` every 2 seconds. Normal jj snapshots are allowed. The plugin must not edit `.jj` directly or create jj bookmarks or refs.
- Key review state by local repository, stable jj change ID, and path. Store one atomic state file per path. Concurrent writes to the same path use shared last-write-wins behavior.
- Store the full jj commit ID when a file is marked reviewed. Use path-filtered `jj interdiff --from <baseline> --to @ --git` for later changes. A missing baseline resets the file to unreviewed.
- File states are unreviewed, reviewed, and changed since review. Moving to another change loads its state; returning to a change resumes its review.
- The file list is on the left and the colored unified diff is on the right. `Tab` changes focus, Vim keys navigate, `v` selects lines, `Enter` inserts a minimal valid unified diff excerpt into the most recently focused agent chat, and `Space` toggles reviewed state.
- Provide explicit `open`, `close`, and `toggle` plugin actions. Do not auto-open the pane.
- Review state is local to one machine.
- Research assets:
  - [Tidewave review-state cache](../research/tidewave-review-state.md)
  - [jj-native review baselines](../research/jj-review-baselines.md)
  - [Diff range representation](../research/diff-range-representation.md)

## Decisions so far

- The complete specification is in
  [implementation-specification.md](implementation-specification.md).
- All research and prototype tickets are complete.
- Herdr v1 has no draft-input API. The approved MVP uses `pane.send_text`
  without a submit key. The user adds a comment after the inserted excerpt.
- The minimum Herdr version is 0.7.5.

## Not yet specified

- None for the MVP.

## Out of scope

- Branch and pull-request review.
- Git-only repositories.
- Stored comments and draft comments.
- Cross-machine review-state synchronization.
- Rename tracking across review baselines.
- Mouse controls, search, configurable keys, themes, and syntax highlighting beyond unified-diff colors.
- Custom large-file limits.
