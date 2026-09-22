# Using the reviewer

See the [README](../README.md) for installation and opening the reviewer.
Press `?` in the reviewer for the full shortcut list.

## Review files and generate guides

Select a file to inspect its diff. Press `Space` to mark it as reviewed;
subsequent changes appear as a new diff since your last pass. Use `[f` and `]f` to
move between unreviewed files, and `[h` and `]h` to move between changed hunks.

Press `rf` to ask the active agent for a guide to the selected file, or `ra` for
all visible unreviewed files. Navigate guide comments with `[r` and `]r`.
Guides and review comments use the same agent: the last focused
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
`Ctrl-t` cycles Files, Threads and Explore while preserving text being composed.

`F2` switches between Vim and regular editing. In Vim mode, `Esc` returns to
normal mode and `i` starts inserting. Arrows, Page Up/Down, `j`/`k` and
`Ctrl-d`/`Ctrl-u` navigate wrapped text according to the current editing mode.

## Browse threads

Use the **Files | Threads | Explore** tabs, or press `f` or `t` for Files or
Threads. Uppercase works too.
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

## Explore a change (experimental)

Open **Explore** and click **Start** or press `s`. Explore prepares the complete
base-to-working-copy change, including reviewed and filtered files. The selected implementation agent investigates the change with you in its existing
conversation, asking at most one next question per turn. Requests are sent through
Herdr immediately, including while the agent is working, using the same delivery
as thread comments. Explore pins that conversation for the pass.
The kickoff prompt supplies the review target and interview instructions. The agent
reads files directly, uses Git/jj for the full diff, and posts the first question
with `submit_question`. Each human contribution sends a wakeup containing the exact
selected option's full text and optional comment, plus IDs identifying the original
question and turn. The agent uses its existing conversation and posts the next turn
with `submit_question`, or concludes with `submit_conclusion`; evidence and assessments are not sent back to it.
The agent keeps context in the same conversation. No history dump, repository catalog,
mailbox or file fallback is exchanged. Refresh the agent's MCP connection after upgrading to update its tool catalog.

**Keep code unchanged while continuing a pass, including after reopening.** Explore reads working-copy files
directly and assumes they stay unchanged. Saved decisions describe that investigation; they do not establish that later edits were reviewed. Use New pass when code has changed. It does not snapshot the repository or
pause decisions when source files change. Supporting files are read as needed,
so large unchanged assets do not impose a repository-wide capture limit.

**Explore progress is saved automatically.** Opening the same checkout and logical
review restores its latest pass: exact questions, answers, corrections, agenda,
conclusions, coverage, separate task/reply drafts, choice selection and reading position.
Reopening sends no prompt and never starts implementation. The next normal action
continues with the original native agent conversation, including a resumed instance.
If that conversation is unavailable or cannot yet be identified, history and edits
remain available. A different conversation requires an explicit New pass.

Accepted responses, posted answers and implementation authorization are saved before
acknowledgement or delivery. Editor changes save in the background and flush on a
normal close; an abrupt process death can lose keystrokes still awaiting a save.
Storage errors preserve the original files and stop unsafe Explore changes. Files
and Threads remain usable. Explore supports one agent and one reviewer per repository,
with one saved editor and reading state per pass.

Explore shows one question at a time across the full content width. Its evidence,
answers and recap scroll together. **Previous** and **Next** (or `[` / `]`) visit
question and conclusion history in posting order; **Latest** returns to the newest question or active conclusion. These controls stay
visible while scrolling. **Opening** shows the initial context. **Conclusion** opens the separate conclusion page.
The pinned **Coverage** control shows the share of required changed lines and file-change items
credited to answered questions. Click it for a file overview and open a file's full diff.
Essential and collapsed supporting references count only after you answer that exact question;
explicit **Defer** earns no coverage. An unanswered assignment remains outstanding. The
boxed overview groups files by remaining review work. Its percentage is the share of added
and deleted lines cited by answered questions, counted on both sides of the diff; unchanged
context and metadata do not enter that percentage. Jev-excluded lines remain in the total
but are not counted as explored. **Show file diff** displays the selected file below the
overview and selects it in Files.
Reaching 100% does not end the discussion or mark files by itself.
**Reply** addresses the displayed question; each question keeps its own unfinished
text, selected choice and evidence state.

The conclusion page separates **Summary**, an editable **To be implemented** box,
and **Future work**. Earlier conclusions retain their own edits and replies; only
the current conclusion offers Implement. Only the agreed task list goes in the box. Edit it and click
**Implement** (or press `Ctrl-Enter` while editing it) to authorize the agent to
implement exactly that list. Summary and future work are not added to the request.
Queued delivery can be cancelled; errors keep your edits. On reopening, a request
that was definitely never attempted stays paused. **Send saved implementation request**
sends its originally authorized scope; **New implementation request** uses the current
edited box. A delivered request is not sent again automatically. **Delivery outcome
unknown** means a crash or transport failure may have interrupted confirmation:
check the original agent conversation before deliberately sending a new request.
A sent status confirms delivery, not implementation completion. **Reply** continues the interview
about the conclusion, without authorizing code changes. A valid conclusion marks the pass's
changed files reviewed at its captured checkpoint. Subsequent edits appear in Files against
that baseline. Explore does not resolve ordinary threads.
The preparation state shows the actual pending status and Cancel. Delivery
errors and Retry appear beside the affected turn.

On follow-up turns, the agent's reply appears above the question. The question is
followed by its choices and the text field for optional details.
The **Context**, **Door** and **Blast radius** Markdown sections follow, then the
evidence with **Establishes** and **For your answer** sections. Each starts with a short
summary paragraph, followed by any useful detail directly in the conversation. Context explains
the behavior and unfamiliar terms, with depth adapted to what you already know.
The provisional map stays on the **Opening** page. Every question
offers two to five alternatives plus **None of the above**, with the first selected by default.
Use `Up` / `Down` or `j` / `k`, click a choice, or press its number to select it.
The text field adds optional details to the selected choice; changing the choice keeps
those details. Click **Send**, press `Ctrl-Enter`, or press `Enter` while selecting choices
to submit both together. **None of the above** keeps the inquiry open and can be sent
with or without text. Click the text field or use `Tab` to reach it. The text fields
use the shared comment editor: `Esc` returns to Normal mode in Vim editing and stays
in the field in Regular editing. `Tab` returns to Explore commands. The short recap
shows the agent's interpretation; `x` appends
a correction through the same answer interface. Ask “Why?”, request a caller,
challenge an assumption, or provide new context in the same free-text composer.
The agent replies directly; a factual question or context is not agreement. Conditional decisions retain
their required changes as follow-ups. Free-text interpretation remains something
to inspect and correct, not a semantic guarantee.

| Explore command | Action |
| --- | --- |
| `Up` / `Down` or `j` / `k` | Select an answer, including None of the above |
| `Enter` | Submit the selected answer and optional details when outside the editor |
| `d` | Defer the question |
| `[` / `]` | Previous / next question |
| `e` / `b` | Next evidence reference / primary evidence |
| `m` | Expand map and follow-ups |
| `PageUp` / `PageDown` | Scroll the conversation when it has focus |
| Mouse wheel | Scroll code over a diff; scroll the conversation outside it |
| `Alt-j` / `Alt-k` | Grow / shrink the selected evidence window |
| `Alt-0` | Fit evidence automatically again |
| `c` / `r` | Cancel pending work / explicitly retry |
| `n` | Start a new pass, retaining the previous investigation as history |
| `Tab` | Cycle conversation, evidence and answer focus |

Each accepted new question opens automatically, including after input while waiting.
Any unposted text stays with its original question and is restored through history.
If you are in Files or Threads, Explore selects the new question for your return
without switching panes. Switching Files/Threads/Explore preserves questions, drafts
and code position. Narrow terminals reflow the question and history controls without
changing Files' sidebar.

The initial native diff window fits the primary evidence's wrapped rows plus
context, capped at about half the content height. Larger sources remain fully
scrollable. Each primary snippet explains what it establishes and how that could
change your answer. **Evidence** lists those decision-relevant snippets;
**Supporting sources** keeps additional citations available without expanding the
question's main evidence list. Repeated references to the same range appear once.
Drag the bottom edge or use the resize keys; **Fit evidence** restores automatic
sizing and positions the complete range, including its outline and wrapped lines. Manual
heights and each opened viewer's position, search, selection and comment draft
stay independent. Overlapping ranges share an outline and fit together. Distant
ranges remain separate rather than enlarging the window to include unrelated
code. Non-text or unavailable sources show a compact limitation.

Yellow outlines mean **relevant to this question**. They do not mean accepted,
reviewed or high risk. Each code window retains search, selection, comments, syntax
highlighting and new-side language-server navigation. Use **Primary** to return
after following a definition. Old/deleted-line LSP operations are unavailable;
additional regular working-copy files inside the repository can be opened on demand.
Citations name paths, sides and lines directly. Outside-repository destinations
and non-regular files remain unavailable.
Old-side references open at their old coordinates. A range outside displayed diff
hunks opens the available full base text in the native viewer. It retains historical
coordinates; it is never substituted with current working-copy text.

A new pass retains the old investigation under **Previous pass**; **Latest pass**
returns to the current one. Decisions are never transferred automatically. Invalid responses leave the last usable question and answer
available. MCP validation errors let the agent repair the same pending turn;
an identical retry of an accepted result is acknowledged without replaying it.
Responses are bounded to 1 MiB; exceeding that bound is
reported without truncation. Stored passes can grow across many responses (up to
256 MiB per pass; editor records up to 16 MiB). Corrupt, oversized or unsupported
records report an error and retain their original bytes. Request and answer identities still protect against
cancelled, duplicate or unrelated responses; they do not establish source freshness.

Consequential questions show separate **Door** and **Blast radius** sections.
The first assesses whether effects can actually be undone, including rollback or
rebuild conditions; the second describes plausible harm, propagation and bounds.
Additional reasoning and unknowns appear below their summaries without an expansion button.
**Supporting sources** opens their citations in the same native viewer.
These are evidence-backed agent judgments, not risk scores or guaranteed safety.

The agenda is provisional. Context can add, refine, reorder, retire or supersede
pending inquiries. Retirement keeps the reason and original wording and does not
mean acceptance. A reconsideration flags a prior conclusion without changing the
original decision; deferrals and conditions remain outstanding. New questions
do not imply a fixed total. The reviewer saves conversation and agenda history; the agent retains context in its own conversation.

The expanded map shows those states, prerequisites, entries not yet mapped and scan limitations. Topic
associations are not proof of coverage. Only answered essential/supporting changed regions
receive credit; an early conclusion is rejected with remaining locations. A successful
conclusion marks changed files at the reviewed checkpoint. Explore does not resolve threads.

Setting a nonempty `TYPESAFE_API_KEY` in the reviewer process enables optional Jev
significance checks. The reviewer sends bounded before/after code snippets and relative
paths to TypeSafe AI. Missing or whitespace-only keys make every change required.
Uncertain, failed and oversized checks remain required. The Jev area in expanded Coverage
shows excluded regions and lets you choose **Require review**; this returns an
uncovered exclusion to the required work without erasing earlier answer coverage.
