# jj-native per-file review baselines

Date: 2026-07-30

## Recommendation

Store the full commit ID that `@` has when the user marks a file as reviewed.
Store it in one atomically replaced state file for each:

```text
(canonical repository, stable change ID, repository-relative path)
```

To show only later edits, use:

```sh
jj interdiff --from <full-baseline-commit-id> --to @ --git -- <path>
```

`interdiff` is better than `diff --from` here. It compares two versions of a
change. If the change was rebased after the mark, it excludes unrelated parent
changes. The command accepts path filters.

An exact hidden commit ID is sufficient during normal operation. It is not a
durable retention reference. If review state must survive operation-log
pruning and garbage collection, retain each baseline commit with a ref.
Jujutsu has no hidden bookmark class. A normal bookmark is visible, maps to a
Git branch, and can be pushed. Thus, the smallest MVP is:

- Store the full commit ID only.
- If jj reports that the commit does not exist, mark the file unreviewed.
- Add private retention refs only if this failure occurs in practice or the
  product requires indefinite baseline retention.

For a strict persistence guarantee on the Git backend, create an atomic ref in
a private namespace such as `refs/herdr/review/<state-key>`. This is not a
public jj-native API, but it retains the commit without copying file content
and without adding a jj bookmark. Delete it when its state file is removed.

## Mark operation

1. Run one normal jj command to snapshot the working copy:

   ```sh
   jj log --no-graph -r @ \
     -T 'change_id ++ " " ++ commit_id ++ "\n"'
   ```

2. Verify that the returned change ID is the change under review.
3. Write the full change ID, full commit ID, and path to a temporary file in
   the state directory.
4. Atomically rename the temporary file over the state file.

If the agent writes after step 1, that write belongs to the next snapshot and
appears in the later interdiff. Concurrent marks of the same file use atomic
last-write-wins behavior. Different paths do not write the same state file.

For UI refresh, group paths that have the same baseline commit and run one
`interdiff` command for that group. On file selection, one command for one path
is the smallest implementation.

## Local validation with jj 0.43.0

I created a repository in `/tmp`, added `a.txt` and `b.txt`, and snapshotted
the working copy. The stable change ID stayed:

```text
lvvwyrrkkkwprrzlxwzumpupynuomkmt
```

The exact commit ID changed after a later edit:

```text
mark:    51b002bc702e4f9a262a4137a18aa90247100884
current: 3b9e4aacf3e5382c77ed266a9fcbd587926c5bd5
```

The old commit was hidden but remained directly addressable:

```sh
jj log -r 51b002bc702e4f9a262a4137a18aa90247100884
jj file show -r 51b002bc702e4f9a262a4137a18aa90247100884 a.txt
jj diff \
  --from 51b002bc702e4f9a262a4137a18aa90247100884 \
  --to @ --git a.txt
```

The diff showed only the line added after the mark. `b.txt` produced no diff.
This agrees with the official FAQ: obsolete working-copy versions are hidden,
but commands can use their exact commit IDs.

The test also ran:

```sh
jj op abandon '..@-'
jj util gc --expire now
```

The baseline survived this small test because it was still reachable through
the current change evolution. This is not a retention guarantee. The official
`jj op abandon` documentation states that predecessor versions are discarded
when they become unreachable from operation history. `jj util gc` can then
remove unreachable commits and objects.

## Lifetime and retention

Jujutsu does not promise a fixed lifetime for an unreferenced hidden commit.
The default `jj util gc` threshold is two weeks, but collection also depends on
reachability and operation-log pruning. A user can request immediate
collection with `--expire now`.

An operation ID is not a better baseline:

- `jj --at-op <id>` can read the repository view at that operation.
- The operation itself can be abandoned and garbage-collected.
- Comparing that old view with the current view is less direct than using the
  exact commit ID.

A normal jj bookmark retains a visible commit, but it has unwanted product
effects. Official jj documentation says bookmarks map to Git branches, and
new local bookmarks can be prepared for a later push. It is not a private
metadata facility.

## Delete and rename behavior

A path-filtered diff from a baseline to a deleted path shows a deletion.

Jujutsu rename detection is output-dependent:

- `jj diff --from <baseline> --to @ --summary` detected
  `R {a.txt => c.txt}` in the local test.
- Filtering only `a.txt` showed a deletion.
- Filtering `c.txt` showed the rename in a normal `diff`.
- `interdiff` represented the patch evolution as deletion plus addition.

For the MVP, treat a rename after review as an old-path deletion and a new,
unreviewed path. Following file identity across renames needs a separate
heuristic and is not required for a correct content review.

## Failure modes

- `jj op abandon` plus garbage collection can remove an unreferenced baseline.
  Result: reset that file to unreviewed, or add retention refs.
- A rebase changes the working-copy commit ID. `interdiff` handles parent
  changes, but the stable change ID check must still pass.
- A new current change with the same repository path must not reuse the old
  state. Include the full stable change ID in the key and file content.
- Rename tracking is not stable under path-only state. Treat it as
  delete-and-add for the MVP.
- A normal jj bookmark can leak into logs or pushes. Do not use it as hidden
  plugin state.

## Official sources

- [Jujutsu FAQ: working-copy snapshots and hidden commits](https://docs.jj-vcs.dev/latest/faq/#jj-is-said-to-record-the-working-copy-after-jj-log-and-every-other-command-where-can-i-see-these-automatic-saves)
- [Jujutsu CLI: `interdiff`, `op abandon`, and `util gc`](https://docs.jj-vcs.dev/latest/cli-reference/)
- [Jujutsu Git expert guide: evolution log](https://docs.jj-vcs.dev/latest/git-experts/#the-evolution-log-shows-the-history-of-a-single-change)
- [Jujutsu bookmarks and Git branch mapping](https://docs.jj-vcs.dev/latest/bookmarks/)
- [Jujutsu concurrency and operation-log storage](https://jj-vcs.github.io/jj/latest/technical/concurrency/)
