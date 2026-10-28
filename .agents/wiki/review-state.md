# Review state

Where a repository's review state lives, how the states that each workspace kept before are
merged into it, the backup taken first, and the locks that keep concurrent reviewers from
losing writes. The decision and its alternatives are in
[ADR 0013](../adr/0013-share-review-state-across-the-workspaces-of-a-repository.md).

## Where it lives

- Herdr gives the plugin `~/.local/state/herdr/plugins/herdr.progressive-reviewer` (the state
  root). `settings.json`, `explore-page/`, `git/` and `run-ahead.log` there stay global or per
  workspace; everything about reviews is in `repositories/<parent worktree>-<16 hex>/`, one
  folder per repository, whose `repository.json` names the jj store or Git common directory.
- `cat repositories/*/repository.json` tells which folder is which repository.

## The merge of per-workspace states

- Builds before ADR 0013 kept one folder per workspace at the root, named by 64 hex digits.
  Each open of the pane, and `reviewer-control stats`, merges those of its repository, then
  moves each to `merged-workspaces/<key>/` (a `merged-into` file names the shared folder).
- A folder no repository can place stays at the root for good: an empty one, or one whose
  changes no repository holds any more. Each repository lists the folders that hold only
  other repositories' reviews in its `foreign-workspaces.json`, and never asks jj about them
  again; delete that file to have them checked again.
- jj: `present(<change>/0)` finds a hidden change, a bare change ID or `change_id()` only a
  visible one (jj 0.45). Exclude `root()`: the all-`z` change is in every repository.
- To see what a merge does to a real state without touching it, `cp -a` the state root to a
  scratch directory, then run `HERDR_PLUGIN_STATE_DIR=<copy> target/debug/reviewer-control
  stats` in the repository.

## Backup and restore

- The first merge copies every per-workspace folder to
  `herdr.progressive-reviewer.backups/<UTC time>/`, beside the state root, and records it in
  `migration.json`. A folder changed after that (an earlier build still running) is copied
  again with the shared folder before its next merge. Nothing deletes a backup.
- To restore, follow the backup's `RESTORE.md`: close the review panes, remove
  `repositories/`, `merged-workspaces/`, `migration.json` and `migration.lock`, then
  `cp -a <backup>/. <state root>/` and remove the copied `RESTORE.md`.
- A failed copy merges nothing; the pane shows "Could not back up the review state; nothing
  was merged" and works on the shared state as it is. The next open tries again.

## Concurrent reviewers

- Marks: every read-modify-write of a review's marks runs under `ReviewStore::lock_marks`.
  It is a `flock`, which a process holds per open file: a thread that takes it twice waits
  forever, so take it once, around the read and the write, and never inside a call that takes
  it.
- Threads, drafts and Explore records take their own lock per review; the merge writes
  through the same calls, so it never overwrites a running reviewer's write.
- Another reviewer's writes show at the next read: there is no live refresh of marks or
  threads across reviewers.
