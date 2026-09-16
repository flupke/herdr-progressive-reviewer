# Using the reviewer

See the [README](../README.md) for installation and opening the reviewer.
Press `?` in the reviewer for the full shortcut list.

## Review files and generate guides

Select a file to inspect its diff. Press `Space` to mark it as reviewed;
subsequent changes appear as a new diff since your last pass. Use `[f` and `]f` to
move between unreviewed files, and `[h` and `]h` to move between changed hunks.

Press `rf` to ask the active agent for a guide to the selected file, or `ra` for
all visible unreviewed files. Navigate guide comments with `[r` and `]r`.
Filenames, guides and review comments all use the same agent: the last focused
agent in the Herdr workspace. Focus another agent to switch; pending comments
follow that selection, while completed answers and conversation history remain.

## Post comments and replies

Select a range with the mouse, or press `V`, move, and press `V` again. An editor
opens below the selection. `a` adds a comment at the cursor. Click **Post** or
press `Ctrl-Enter` to publish it. The editor keeps its text until posting succeeds.

Use `A` or **Reply…** to add a follow-up. Posted messages cannot be edited or
deleted; add a reply to correct an earlier message. Agent replies appear in the
same conversation and may address several comments. Click a message to select
it, and use `[c` and `]c` to move between messages.

Unposted comments and replies are saved across restarts without sending them.
**Cancel**, or submitting empty or whitespace-only text, discards the draft.
`Ctrl-t` switches between Files and Threads while preserving text being composed.

`F2` switches between Vim and regular editing. In Vim mode, `Esc` returns to
normal mode and `i` starts inserting. Arrows, Page Up/Down, `j`/`k` and
`Ctrl-d`/`Ctrl-u` navigate wrapped text according to the current editing mode.

## Browse threads

Use the **[F]iles | [T]hreads** tabs, or press `f` or `t`. Uppercase works too.
Threads lists conversations across the whole review, including ones whose
original code has changed or disappeared.

| In Threads | Action |
| --- | --- |
| `1` / `2` | Filter to Unresolved / All |
| `/` | Search messages and file paths |
| Arrows, then `Enter` | Select and read a thread |
| `A` | Reply |
| `r` | Resolve or unresolve |
| `p` | Peek at the current file; `Esc` returns |
| `Alt-u` | Show All and jump to the first thread with unread replies |

Each thread retains its original code context. Click the underlined filename
above it to open the file in Files. Peeking at current code offers highlighting,
search and [language server navigation](language-servers.md); it refreshes when
the file changes. Returning to Files preserves your review location.

## Resolve threads and track replies

**Resolve thread** collapses the inline conversation to a summary with an
**Unresolve thread** button. In Threads, resolving selects the next unresolved
thread, prioritizing one with unread answers. If none remain, the conversation
pane clears. Choose **All** to revisit resolved history.

Reviewed files hide their code diff while keeping conversations accessible.
Resolving a thread does not mark its file as reviewed. It stops requests for an
agent answer; late replies are kept without reopening the thread. Unresolve
makes unanswered comments eligible for delivery again. Use **Retry agent** if
the agent stopped before answering.

File badges show 📄 for guides and 💬 for unresolved conversations. A red `●`
marks unread replies beside the filename and in the Threads tab, including late
replies on resolved threads. A reply is marked read once every part has been
visible, including when you scroll through a reply taller than the pane.

For connection problems or delayed notifications, see
[Agent connection through MCP](mcp.md).
