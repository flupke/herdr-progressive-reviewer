# Delegate a Review Guide

Delegate one progressive-review guide to a fresh subagent. Do not read the
frozen checkpoint diff in your current context, and do not write the guide
response yourself.

1. Start one fresh subagent that inherits the complete current conversation.
   Copy the complete text from `## Review-guide task for the subagent` through
   the request below into its task. The subagent must read the complete frozen
   diff in its own context and write the response.
2. If your agent runtime cannot start a subagent with inherited conversation
   context, invoke `$handoff` to capture the high-level implementation intent,
   decisions, feedback, and later edits. The handoff must reference the frozen
   diff path and must not copy the diff. Start a fresh subagent with the
   handoff path and the same complete review-guide task.
3. Wait for the subagent to finish. Do not edit the repository.

## Review-guide task for the subagent

Write a guide for one progressive review. The request at the end of this
prompt gives you:

- the repository root;
- the exact frozen checkpoint diff to explain;
- the requested scope;
- temporary and final response paths.

Use the inherited conversation, or the fallback handoff, to understand the
implementation intent, decisions, feedback, and later edits. Use the frozen
diff as the authority for changed content, paths, and hunk identifiers. The
current repository can be newer than the checkpoint diff. Read it only when
surrounding source is necessary to understand the frozen diff.

Write an author's guide, not a correctness review. Do not edit the repository.

## Find the central ideas

Read the complete frozen diff for the requested scope. Use the inherited
conversation or fallback handoff to understand the intended behavior, central
design decisions, constraints, feedback, and edits. Use the diff to find where
those ideas appear in the change. Cover each distinct design boundary, data
flow, safety constraint, and non-obvious decision that a reviewer must
understand. Do not add an item only because a file or hunk changed.

## Write focused guide items

Each item must explain one distinct idea that is difficult to understand from
the code. Do not combine separate ideas only to reduce the item count. Large
or cross-cutting changes will usually need several items. An empty guide is
valid only when the complete diff is simple and needs no explanation.

Use concise and simple text. Avoid jargon. Use more than one paragraph only
when it is necessary to explain the idea.

Each item has one target:

- Use a `lines` target when an idea covers a smaller part of a hunk. This is
  especially important for large hunks and new files that contain several
  distinct ideas. `old` and `new` are optional one-based inclusive line
  ranges. Supply at least one side. Supply both sides for a replacement when
  both ranges help identify the changed region. Make each range as small as
  possible. Its first line must be the first line of code that the text
  explains. Its last line must be the last line of code that the text
  explains. Do not include nearby declarations, comments, blank lines, or
  parts of another operation only to give the item more context. When one
  item explains several operations, include the complete consecutive region
  from the first explained operation through the last one.
- Use a `hunks` target for one or more consecutive diff hunks that together
  show the idea. `first_hunk` and `last_hunk` form an inclusive range. Use the
  same value for both fields when the item explains one complete hunk.
- Use a `file` target only when the change has no text hunk, such as a
  rename-only or binary change.

Paths must be exact repository-relative paths from the frozen diff. Hunk
identifiers are the one-based identifiers shown for that file in the frozen
diff. Line numbers must be exact line numbers from the indicated old or new
side of that diff. A target must not include unrelated changed content only to
connect separate ideas. Targets must not overlap.

## Write the response

Write one JSON document with this shape:

```json
{
  "schema_version": 1,
  "items": [
    {
      "target": {
        "kind": "lines",
        "path": "crates/example/src/lib.rs",
        "old": null,
        "new": {
          "first_line": 40,
          "last_line": 72
        }
      },
      "text": "This parser keeps request validation separate from storage."
    },
    {
      "target": {
        "kind": "hunks",
        "path": "crates/example/src/lib.rs",
        "first_hunk": 2,
        "last_hunk": 3
      },
      "text": "This boundary lets Git and jj use the same review state."
    },
    {
      "target": {
        "kind": "file",
        "path": "assets/model.bin"
      },
      "text": "The application reads this model when it processes input."
    }
  ]
}
```

Write the complete document to `{{RESPONSE_TEMPORARY_PATH}}`. After the write
succeeds, atomically rename it to `{{RESPONSE_PATH}}`. Do not write the final
path directly. Do not write any other output file.

## Request

- Repository root: `{{REPOSITORY_ROOT}}`
- Scope: `{{REVIEW_SCOPE}}`
- Frozen checkpoint diff: `{{DIFF_PATH}}`
- Temporary response: `{{RESPONSE_TEMPORARY_PATH}}`
- Final response: `{{RESPONSE_PATH}}`
