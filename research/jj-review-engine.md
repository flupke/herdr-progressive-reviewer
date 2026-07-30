# jj review engine protocol

Date: 2026-07-30

## Decision

Use normal jj commands for all repository reads. The first command in each
two-second poll is `jj status`. It snapshots the working copy as required.
After that command completes, read one exact identity for `@` and build all
views from that identity. Do not read or edit `.jj`.

Use `--color=never --no-pager` for parsed commands. Use `--` before all path
filesets. Pass arguments as an argv array. Do not build a shell command.

## Poll transaction

Run these commands from the canonical result of `jj root`:

```sh
jj --color=never --no-pager status
jj --color=never --no-pager log --no-graph -r @ \
  -T 'change_id ++ "\0" ++ commit_id ++ "\0"'
jj --color=never --no-pager diff -r <full-commit-id> --summary
```

The template must return full IDs. The parser requires exactly two non-empty
NUL-delimited fields. The second command gives the poll snapshot ID. Later
commands use this full commit ID instead of `@`.

After the data loads, run the identity command again with
`--ignore-working-copy`. If its commit ID differs, discard the result and start
the next poll. This prevents a mixed view when another process changes the
working copy during the poll.

When the stable change ID differs from the displayed change ID, load the state
for the new change and reset UI selection and scroll positions. When the user
returns to an old stable change ID, load its prior records.

`jj status` text is for user diagnostics only. Do not parse it as the file
protocol.

## File list

Use a tree-diff template instead of parsing the human `--summary` format:

```sh
jj diff -r <commit-id> -T \
  'source.path() ++ "\0" ++ target.path() ++ "\0" ++
   source.file_type() ++ "\0" ++ target.file_type() ++ "\0"'
```

During implementation, validate the exact optional-value syntax against the
minimum jj version and lock it with fixture tests. If the minimum supported jj
does not expose a stable template for one field, use `--summary` only for that
field and test all status letters.

The normalized file record is:

```rust
struct ChangedFile {
    old_path: Option<RepoPath>,
    new_path: Option<RepoPath>,
    old_kind: FileKind,
    new_kind: FileKind,
    change: ChangeKind,
    display_path: String,
}

enum ChangeKind { Added, Modified, Deleted, Renamed, TypeChanged, Conflict }
enum FileKind { Absent, File, Symlink, Submodule, Conflict }
```

Paths are repository-relative byte strings. The UI uses escaped display text.
State uses the selected current path, or the old path for a deletion. A rename
in the current change can display as one row. A rename after a review baseline
is a deleted old path plus a new unreviewed path.

Sort by escaped display path. File state does not change the sort order.

## Full diff

For the selected file, run:

```sh
jj --color=never --no-pager diff -r <commit-id> --git -- <path>
```

For a rename, include both old and new paths as separate argv values. Parse the
Git-style unified diff into:

```rust
enum DiffRow {
    FileHeader { old_path: Option<RepoPath>, new_path: Option<RepoPath> },
    Meta { text: String },
    Hunk { old_start: u32, old_count: u32, new_start: u32, new_count: u32 },
    Context { old_line: u32, new_line: u32, text: String },
    Delete { old_line: u32, text: String },
    Add { new_line: u32, text: String },
    Notice { kind: NoticeKind, text: String },
}

enum NoticeKind { Binary, Conflict, Submodule, Unsupported }
```

Keep the raw line text for excerpt output. Validate each hunk row count. If the
parser cannot classify output, show the raw metadata as a notice. Do not crash
or mark the file reviewed.

Use these colors:

- additions: green;
- deletions: red;
- hunk headers: cyan;
- file and metadata headers: dim;
- conflict and parse notices: yellow.

## Review state

For an unreviewed file, the right pane shows the full current-change diff.

When the user marks a file reviewed:

1. Run the identity command without `--ignore-working-copy` so jj snapshots.
2. Require the same stable change ID as the displayed change.
3. Store its full commit ID in the path record.
4. Show no diff for that path until it changes.

For each reviewed record, first verify the baseline:

```sh
jj --color=never --no-pager log --ignore-working-copy --no-graph \
  -r <baseline-commit-id> -T 'commit_id ++ "\0"'
```

Then run:

```sh
jj --color=never --no-pager interdiff --ignore-working-copy \
  --from <baseline-commit-id> --to <current-commit-id> --git -- <path>
```

An empty result means `reviewed`. A non-empty result means
`changed since review`. Parse it with the same diff parser.

If the baseline cannot resolve, atomically delete the stale record from active
state and show `unreviewed`. Do not run a content diff as a fallback.

Pressing `Space` on a reviewed or changed-since-review file clears its record
and makes it unreviewed. Pressing `Space` on an unreviewed file marks it
reviewed.

## Special output

| Case | Required result |
| --- | --- |
| Empty change | Empty file list with `No changes in current change`. |
| Binary file | Selectable row and `Binary file; text diff is unavailable`. |
| Conflict | Selectable row, conflict notice, and parseable text when present. |
| Deletion | Old path is the state path; full deletion diff is visible. |
| Rename in current change | One display row when jj reports a rename. |
| Rename after baseline | Old path becomes changed/deleted; new path is unreviewed. |
| Symlink or submodule | Metadata notice; never decode the target as file text. |
| Invalid UTF-8 path | Lossless internal bytes and escaped UI text. |
| Command timeout | Keep last good model and show a stale/error marker. |
| Missing baseline | Delete record and reset to unreviewed. |
| Change-ID switch | Load the other change state before drawing its statuses. |

Set a five-second command timeout. Kill and reap a timed-out child. Keep the
last complete snapshot. Never publish a partial poll.

## Validated facts

Local validation used jj 0.43.0.

- Normal commands snapshot the working copy.
- `jj diff` accepts `--git`, `--summary`, templates, and path filesets.
- `jj interdiff` accepts full `--from` and `--to` revisions, `--git`, and path
  filesets.
- `interdiff` compares patch evolution and excludes unrelated parent changes
  after a rebase.
- A hidden predecessor can resolve by its full commit ID until jj removes it.

The fixture suite must use both pure and colocated repositories. It must cover
add, modify, delete, rename, binary, conflict, rebase, missing baseline, and a
change switch.

## Sources

- [Jujutsu CLI reference](https://docs.jj-vcs.dev/latest/cli-reference/)
- [Jujutsu templates](https://docs.jj-vcs.dev/latest/templates/)
- [Jujutsu working-copy snapshots](https://docs.jj-vcs.dev/latest/faq/#jj-is-said-to-record-the-working-copy-after-jj-log-and-every-other-command-where-can-i-see-these-automatic-saves)
- [Local baseline validation](jj-review-baselines.md)
