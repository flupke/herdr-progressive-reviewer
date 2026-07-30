# Concurrent review-state storage

Date: 2026-07-30

## Layout

Use the Herdr-provided state directory when it exists. For standalone tests,
use `${XDG_STATE_HOME:-$HOME/.local/state}/herdr/progressive-reviewer`.

```text
<state>/
  <repository-key>/
    changes/
      <full-change-id>/
        paths/
          <path-key>.json
```

`repository-key` is lowercase SHA-256 of the canonical UTF-8 bytes from
`jj root`. This makes the identity local to one workspace path. Moving the
workspace starts new local state. A symlink to the same root resolves to the
same key.

`path-key` is lowercase SHA-256 of the exact repository-relative path bytes.
Do not put an encoded path directly in a file name. The record contains the
path and detects a hash collision.

Full jj change IDs and commit IDs are lowercase hexadecimal values. Reject
other text before it becomes a directory or command argument.

## Path record

Each reviewed path has one file:

```json
{
  "schema_version": 1,
  "change_id": "<full jj change id>",
  "path_encoding": "utf8",
  "path": "src/lib.rs",
  "baseline_commit_id": "<full jj commit id>",
  "reviewed_at": "2026-07-30T20:00:00.000000000Z"
}
```

On Unix, if a path is not valid UTF-8, use:

```json
{
  "path_encoding": "base64",
  "path": "<standard base64 without line breaks>"
}
```

Hash the original bytes in both cases. Normalize neither Unicode nor path
case. Reject absolute paths, empty components, `.` components, and `..`
components.

`reviewed_at` is for diagnostics. It does not decide write order. Shared
last-write-wins means the record from the last successful rename is the
complete value.

No file means `unreviewed`. A valid file plus an empty interdiff means
`reviewed`. A valid file plus a non-empty interdiff means
`changed since review`.

## Atomic replacement

For a mark operation:

1. Create the target directory with mode `0700`.
2. Create a new temporary file in the same directory with
   `create_new`, mode `0600`, and a random name.
3. Write the complete compact JSON value plus one newline.
4. Flush the file and call `fsync` on it.
5. Rename the temporary file over `<path-key>.json`.
6. Call `fsync` on the containing directory on systems that support it.
7. Remove an abandoned temporary file after any error.

Never truncate the target in place. A reader opens the target, reads it to
EOF, and validates all identity fields before use. Rename in one directory is
the commit point.

For an unreview operation, unlink the exact path record and sync the parent
directory. An absent file is success.

There is no read-modify-write operation across multiple records. Different
paths never share a writable file. Two writers for the same path can both
complete. The last rename wins.

## Reader behavior

Readers do not lock. They can see the old complete record or the new complete
record. They must not see a partial record.

On each two-second refresh, stat loaded records. Reload a record when its
identity, size, or modification time changes. A full directory rescan is also
acceptable for the MVP because the current change has a small bounded file
set.

When another pane writes the same path, the next poll uses that record. Thus,
two panes share review progress.

## Invalid and stale state

| Condition | Behavior |
| --- | --- |
| Unknown schema version | Ignore the record and show one warning. |
| Invalid JSON or identity mismatch | Ignore it; rename it to `*.invalid-<time>` when safe. |
| Missing baseline commit | Delete the active record and show unreviewed. |
| Path is no longer in the change | Keep the record for return to this change. |
| Change is not current | Keep its directory; load it when the change returns. |
| Repository moved | Use a new repository key. Old state is stale local data. |
| Temporary file after a crash | Ignore it and remove it after 24 hours. |
| Hash collision | Reject the record and do not overwrite it. |

The MVP has no automatic expiry for valid records. The data is small and is
needed when the user returns to a change. A later maintenance command can
remove repository directories whose root no longer exists and change
directories that have not been read for a configured period.

## Security limits

- Open directories and files without following a target symlink when the
  platform supports this.
- Validate every ID and path before joining it to the state root.
- Do not store repository file content.
- Do not store agent chat text.
- Do not use a jj bookmark or ref for retention.
- Set user-only permissions because local paths can be sensitive.

## Acceptance checks

- Two process tests repeatedly write different valid records to one path.
  Every read is one complete valid record.
- Writes to two paths never replace each other.
- A killed writer before rename leaves the previous record active.
- A killed writer after rename leaves the new record active.
- An unreview racing with a mark has last-operation-wins behavior.
- UTF-8 and non-UTF-8 paths round-trip and produce stable keys.
- A changed root path produces a different repository key.
- Missing baselines reset only their own path records.
