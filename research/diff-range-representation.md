# Diff range representation

Date: 2026-07-30

## Recommendation

Use a short, side-aware reference in agent chat:

```text
doc/foo.txt:12:30
doc/foo.txt:old:8:11
```

Parse the numeric fields from the right so a colon in a path does not make the
reference ambiguous. Line numbers are 1-based and inclusive.

- `path:start:end` means lines in the current file. This is the default because
  the agent can read these lines directly.
- `path:old:start:end` means lines in the parent side of the current jj change.
- For a mixed selection, insert one current-side reference and one old-side
  reference. Do not compress it into one range.
- For one line, use the same form with equal endpoints. This keeps the parser
  and producer small.

For example, a replacement selection can insert:

```text
doc/foo.txt:old:12:14 doc/foo.txt:12:13
```

This is the smallest format that keeps the requested `path:12:30` form and
also identifies deletion-only and mixed selections without guessing.

This chat reference is not a durable anchor. It means “these lines in the diff
that the reviewer currently sees.” It is sufficient when the plugin inserts
the reference and the user immediately sends a message to the local agent.

If comments or drafts are stored for later use, store a separate structured
anchor:

```text
change_id
base_commit_id
head_commit_id
old_path
new_path
start: { side, line }
end:   { side, line }
selected_diff_text
```

The exact base and head commit IDs identify the diff version. The selected
text gives a fallback for display and relocation. Do not use unified-diff row
offsets as the durable identity.

## Why one line range is not sufficient

A unified diff has two line-number spaces.

The hunk header has this shape:

```text
@@ -old_start,old_count +new_start,new_count @@
```

A context row advances both counters. A deletion advances only the old
counter. An addition advances only the new counter. The [GNU Diffutils
manual](https://www.gnu.org/software/diffutils/manual/html_node/Detailed-Unified.html)
defines the two hunk ranges and the space, `-`, and `+` row markers.

Thus:

- context plus additions can use one current-side range;
- deletion-only selections need an old-side range;
- a selection that contains deleted and added rows has coordinates on both
  sides and cannot be represented exactly by `path:start:end`;
- a diff-row ordinal can cover all visible rows, but it changes when hunks or
  context change.

Hunk coordinates describe the complete hunk. They do not identify a selected
subset of its rows. They are useful when parsing the diff, not as the chat
locator.

## Existing systems

### GitHub

GitHub stores the file path and exact commit ID. A multiline review comment
uses the last endpoint as `line` plus `side`, and the first endpoint as
`start_line` plus `start_side`. `LEFT` means a deletion. `RIGHT` means an
addition or unchanged context. The endpoint sides make the model capable of
describing a range whose endpoints are on different sides.

GitHub's older `position` field is a row count from the first `@@` header. It
continues through later hunks until the next file. GitHub is closing down this
field and tells clients to use line and side fields. This is good evidence
against storing a raw unified-diff row offset.

Source: [GitHub pull-request review comment
API](https://docs.github.com/en/rest/pulls/comments#create-a-review-comment-for-a-pull-request).

### GitLab

GitLab stores `base_sha`, `start_sha`, and `head_sha`, both old and new paths,
and old/new line coordinates. A multiline position has structured `start` and
`end` values. Each endpoint includes:

- `type`: `old` or `new`;
- `old_line` and/or `new_line`;
- a `line_code`.

GitLab's official response example starts on a `new` endpoint and ends on an
`old` endpoint. This is an explicit mixed-side representation. For a
single-line note, GitLab says to use only `new_line` for an addition, only
`old_line` for a deletion, and both for unchanged context.

Source: [GitLab Discussions
API](https://docs.gitlab.com/api/discussions/#create-a-new-thread-in-the-merge-request-diff).

### Gerrit

Gerrit stores a comment path, commit ID, and one side: `REVISION` or `PARENT`.
Its optional range has 1-based start and end lines plus 0-based character
positions. The start is inclusive and the end is exclusive. Because `side`
belongs to the complete comment, one Gerrit range does not describe a
mixed-side selection.

Gerrit's model is useful for source-text ranges inside one version. It is not
the best model for a visual selection that crosses deleted and added rows.

Source: [Gerrit CommentInfo and
CommentRange](https://gerrit-review.googlesource.com/Documentation/rest-api-changes.html#comment-info).

### herdr-reviewr

`herdr-reviewr` keeps each comment in memory with:

```text
file, side, start, end, selected diff lines, comment text
```

Its selection code uses new-side line numbers if any selected row has one.
Only a pure deletion uses the old side. It keeps all selected rows, with their
diff markers, as a snippet. Its exported human location is
`path:start-end`; old-side locations add ` (removed)`.

For a mixed deletion/addition selection, the short location therefore names
only the new-side span. The preserved snippet carries the deleted part. This
is acceptable for its export format because the snippet is always included.
It would lose information if copied as a location alone into agent chat.

The project specification also says that comments live only for the session,
that the selected diff lines are the authoritative anchor, and that side and
line numbers are not rebound when the diff shifts.

Sources:

- [`anchor` source](https://github.com/persiyanov/herdr-reviewr/blob/main/src/app.rs)
- [review model specification](https://github.com/persiyanov/herdr-reviewr/blob/main/specs/review-model.md)

## MVP behavior by selection type

| Selected rows | Chat text | Reason |
| --- | --- | --- |
| Context only | `path:12:30` | Context exists in the current file. |
| Additions only | `path:12:30` | Additions exist in the current file. |
| Context and additions | `path:12:30` | One current-side interval is exact. |
| Deletions only | `path:old:12:30` | Deleted lines have no current-file number. |
| Deletions and additions | Two references | One interval cannot preserve both sides. |
| Disjoint rows | One reference per contiguous run | A bounding interval would include rows the user did not select. |

In a mixed selection, derive the old and current bounding ranges from only the
rows that have coordinates on that side. Context rows have both coordinates.
Include a context row in the side where it keeps the selected run contiguous;
do not duplicate it unless both runs need it to stay understandable.

The agent must interpret `old` as the parent side of the current jj change,
not as the current filesystem. The plugin can include this convention once in
its agent integration prompt.

## Durable storage versus chat

The systems above all add version identity when a comment must survive later
changes. Line numbers alone drift when code is inserted or removed earlier in
the file.

For this MVP, pressing Enter can insert only the human-readable reference.
The reference does not need its own state file if the agent chat owns it
immediately.

If the plugin later stores pending comments, use one file per comment or draft
and store the structured versioned anchor. This is separate from the
per-file reviewed baseline. A reviewed baseline answers “what changed after
review?” A comment anchor answers “which displayed code did this note refer
to?” They must not share one lossy line-range field.
