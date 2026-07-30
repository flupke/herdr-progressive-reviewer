# Ratatui review flow prototype

Date: 2026-07-30

## Screen model

The MVP uses one full-screen alternate-buffer Ratatui application.

```text
 Progressive review · change qpvuntsm · 3/5 reviewed
┌ Files (focus) ───────────┬ Diff · src/lib.rs ──────────────────────┐
│ ○ README.md              │ diff --git a/src/lib.rs b/src/lib.rs    │
│ ✓ src/app.rs             │ @@ -12,3 +12,4 @@                       │
│ ● src/lib.rs             │  fn run() {                             │
│ ! assets/logo.bin        │ -    old();                             │
│                          │ +    new();                             │
├──────────────────────────┴─────────────────────────────────────────┤
│ Tab focus · j/k move · v select · Enter insert · Space review     │
│ Changed since review · agent: codex-api                            │
└────────────────────────────────────────────────────────────────────┘
```

State markers are:

- `○` unreviewed;
- `✓` reviewed;
- `●` changed since review;
- `!` conflict, binary, parse, or command notice.

Color is additional information. The marker and status text must be clear
without color.

The file pane uses 30 percent of the width, clamped to 24 through 48 columns.
The diff pane uses the remainder. The footer uses two rows when space permits.

## Focus and keys

`Tab` changes focus between files and diff. A visible border and title suffix
show focus.

| Key | File focus | Diff focus |
| --- | --- | --- |
| `j`, Down | Next file | Next visible row |
| `k`, Up | Previous file | Previous visible row |
| `g` | First file | First diff row |
| `G` | Last file | Last diff row |
| `Ctrl-d` | Half page down | Half page down |
| `Ctrl-u` | Half page up | Half page up |
| `v` | No action | Start or finish visual line selection |
| `Esc` | Clear notice | Cancel selection |
| `Enter` | Focus diff | Insert selected excerpt |
| `Space` | Toggle selected file review | Toggle current file review |
| `q` | Quit | Quit |

Navigation wraps neither files nor diff rows. When the selected file changes,
the diff row starts at the first hunk. The TUI retains each file scroll
position until a new jj change ID loads.

`Space` is disabled while a mark command is in flight. The footer shows
`Marking reviewed…`. A failure leaves the prior state.

## Visual selection

Press `v` on a selectable diff row to set the anchor. Move with normal keys.
The inclusive range between anchor and cursor is highlighted. File headers,
metadata, and one or more complete hunk headers that apply to selected content
are included automatically in the excerpt, but they are not selectable
content.

Press `v` again to keep the range selected. `Esc` clears it. A new file clears
it.

`Enter` requires at least one context, add, or delete row. It builds the
smallest valid unified diff:

```diff
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -12,2 +12,2 @@
-    old();
+    new();
```

For each selected contiguous run:

1. Include the file headers.
2. Include one recomputed hunk header.
3. Include selected content rows.
4. Include no unselected context unless one context row is necessary to make a
   zero-length side range understandable.
5. Recompute old and new start and count values from parsed coordinates.
6. Keep diff markers and exact text.

If a selection crosses hunks, output one valid hunk for each run. If it crosses
files, reject it because the MVP shows one file at a time. Binary and metadata
notices cannot be inserted as a code excerpt. The footer explains why.

After a successful insert, clear the selection. After failure, keep it.

## Small terminals

At widths below 72 columns, use one pane at a time:

- file focus shows the file list;
- diff focus shows the diff;
- `Tab` changes the view.

At heights below 10 rows, show one header row, one footer row, and the active
pane. Shorten paths in the middle and keep their last component visible.

Below 40 by 6, replace the normal UI with:

```text
Terminal is too small
Minimum: 40x6
q quit
```

All controls remain keyboard-only. The MVP has no mouse handling.

## Notices

- Binary: `Binary file; text diff is unavailable`.
- Conflict: `Conflict content. Resolve with jj before final review`.
- Missing baseline: `Review baseline expired; file reset to unreviewed`.
- Poll error: `Refresh failed; showing data from <time>`.
- No agent: `No agent chat is available in this workspace`.

Notices use the footer and do not replace the file list. A long notice can
truncate. The full text is available on the next redraw when space permits.

## Prototype acceptance script

Use fixture diffs with at least 200 files and a 10,000-row file.

1. Navigate both panes with Vim keys and arrows.
2. Select additions, deletions, replacements, and ranges across two hunks.
3. Parse each generated excerpt with `git apply --check` against its correct
   fixture base when the excerpt is applicable.
4. Confirm that review markers do not depend on color.
5. Test 120x30, 72x15, 60x10, 40x6, and 39x5 terminals.
6. Confirm that a binary row and conflict row give notices without a panic.
7. Confirm that an insertion failure keeps the selection.
8. Confirm that a change-ID switch resets transient navigation but restores
   stored review state.

## Implementation boundary

The prototype is a pure state machine with `update(event)` and `view(model)`.
Repository commands and Herdr calls run outside the render path and send typed
messages back to the state machine. This permits deterministic key and screen
tests with Ratatui `TestBackend`.
