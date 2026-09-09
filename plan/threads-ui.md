# Threads UI and UX

Implement the supplied [design](https://chatgpt.com/s/t_6aa40516ff3c8191bbb89c60fb9f09ee)
and [HTML prototype](herdr-thread-navigation.html). The [design text](reviewer-ux-design.md)
is also saved here. Reference views:
`reviewer-ux-{files,threads,reviewed,peek,removed,reply}.png` in this directory.

The later MCP decisions supersede the draft/send queue in the prototype:
Post publishes immediately and posted messages are immutable. Remove Ctrl-s
entirely, including the retry hint and posting shortcut. Use Post or Ctrl-Enter
to submit, and a follow-up message to ask the agent to revisit a thread.
Preserve unposted composer text while navigating.

- Files and Threads share the existing sidebar and main pane; no third pane.
  Put navigation hints in the tab titles, `[F]iles | [T]hreads`, with `f`/`F`
  opening Files and `t`/`T` opening Threads outside input fields. Keep `Ctrl-t`
  as a toggle while composing. Remove the Back to files / Tab panes detail hint.
- The thread list covers the whole logical review, one entry per thread, with
  opening question, path, attention state and context notice. Use separate bordered
  cards. Show search across messages and paths at the top only while editing or
  applying a query; `/` opens it. Omit the idle search hint and empty-list messages.
  Pin the two filters,
  Unresolved and All, to the bottom of the pane, with 1/2 shortcuts and mouse
  selection. All retains resolved history. Alt-u selects All and the first unread
  thread. Preserve stable ordering.
- Select a thread independently of file selection. Match the inline diff thread
  rendering: compact original code with syntax colors, line numbers and change
  gutters, followed by the same chronological message layout and reply editor.
  Above the code, show an underlined file path that opens the matching file in
  Files when clicked, followed by a horizontal divider. Omit line-range
  suffixes, checkpoint/context descriptions and the empty unread-status message.
  Keep the reply editor directly after short conversations without blank filler.
  Pin it below the scrolling content only when the conversation fills the pane.
  Opening an inline reply scrolls Files only enough to reveal the editor bottom,
  including its Post/Cancel controls; leave the viewport unchanged when it fits.
  While composing, Cancel, Post and Resolve/Unresolve share one right-aligned
  row when space permits. Only Post uses the green default-action background;
  Cancel and Resolve/Unresolve use the neutral button background.
  Keep the Reply control. Show Resolve/Unresolve at the bottom right of each
  thread box in Files and Threads. Omit the Peek current file button; keep the
  keyboard shortcuts `r` and `p`. Opening
  does not start composing. This later user feedback supersedes question-first
  standalone rendering in the original prototype.
  Use the same black terminal background as Files/Diff throughout the Threads
  view, including message cards and the reply editor. Keep the shared inline
  thread renderer consistent.
- Threads must fit the visible pane immediately after opening, without a manual
  resize or tab switch. Synchronize the plugin PTY geometry after open/focus;
  Herdr 0.9 can retain the parent pane's dimensions. A zero-distance `pane.resize`
  request applies the current split geometry while preserving its ratio and focus.
- Remove `dc` and the posted-message deletion operation. Resolve thread collapses
  the inline thread to a separate summary box with an Unresolve button, without
  enclosing its code range. The code remains ordinary file diff. Retain messages,
  context and unposted text; expanding restores them. Collapsed replies stay unread.
- Keep resolution and the reviewer's last-seen reply marker durable and separate
  from agent retrieval and file review state. New replies never resolve threads
  or change file review status. Late replies on resolved threads remain findable.
- Keep thread and unread badges on reviewed files. Never show them on directories,
  whether expanded or collapsed. Put 📄 guide and 💬 badges beside filenames;
  show 💬 only when unresolved conversations remain and omit the counter. A red dot beside names
  and in the Threads tab title indicates unread replies, including late replies
  on resolved threads. Reserve badge width before truncating filenames;
  compact directory chains.
- Keep file progress independent of thread status; omit aggregate thread counters
  from the footer. Reviewed files hide the code diff, including cached rows, while showing inline
  conversations and reply fields (or resolved summaries). Empty diffs also retain threads,
  rather than summaries that require opening the Threads tab.
- Return to the previous file, cursor, scroll, checkpoint and focus. Source peeks
  are read-only and never unreview files or replace original thread context.
  Always show the full saved diff range without expansion controls. Peek uses
  the same native source viewer as the diff panel, including syntax highlighting,
  search, navigation and LSP; Esc returns to the conversation and its draft.
- Distinguish hidden/folded context, earlier/unmapped code, a file absent from the
  displayed diff, and an actually unavailable current file. Keep original context
  usable when files are removed; use verified rename paths when available.
- Validate keyboard and mouse navigation, narrow/wide layouts, review switching,
  resolution/read persistence, incoming replies without focus/scroll jumps, and
  editor preservation. Run make check, both code-review axes, describe-commit and
  make install against the MCP change as the fixed point.
- Prevent review notifications from appending to unfinished agent input. Defer
  while the pane is focused or the visible composer is nonempty or unrecognized,
  show a notice once, and retry without changing input. Recheck focus after the
  screen read. Test with real isolated Herdr servers for Codex and Claude. The
  empty-input check must recognize Codex's dim placeholder and animated composer
  from ANSI styling, while rejecting typed placeholder text, multiline drafts
  and attachments. Plain text alone cannot distinguish the placeholder from input.
  The installed Herdr API has no atomic empty-composer submission, so document the
  remaining race between checking and submission.
- Wait for Herdr to identify the selected native agent session before issuing
  review access. Preserve queued comments during detection;
  another detected agent must not take over queued work. Keep known session
  bindings strict. Returning to idle after an unsuccessful MCP attempt must not
  repeat the same notification; new comments and follow-ups still wake the agent.

- Clear unread reply indicators automatically once every part of that reply has
  been visible in either Files or Threads. Accumulate coverage while scrolling
  through replies taller than the pane. Opening/selecting alone does not count;
  hidden panes, peeks and text behind overlays do not count. Persist read status
  per reply so an unseen earlier reply or a newly arrived reply remains unread.

- Send UI thread commands and agent focus/status changes directly to the conversation
  worker, alongside MCP requests. Repository refreshes must not delay saved posts
  past the agent's final fetch or hold back the next idle notification. Comments
  posted after that fetch still trigger a new notification without UI navigation.

- Avoid idle redraws. Repaint on input and background changes, and on timer ticks
  only for a deferred file load, guide animation or a toast deadline. Suspend
  notification polling when no reviewer messages are waiting for the agent.
