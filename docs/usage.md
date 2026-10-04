# Using the reviewer

See the [README](../README.md) for installation and opening the reviewer.
Press `?` in the reviewer for the full shortcut list.

## Review files

Select a file to inspect its diff. Press `Space` to mark it as reviewed;
subsequent changes appear as a new diff since your last pass. Use `[f` and `]f` to
move between unreviewed files, and `[h` and `]h` to move between changed hunks.

To review a file hunk by hunk, click the `☐` in a hunk's top-right corner, or
press `rh` with the cursor in it. The hunk folds into one `✓ reviewed hunk` row,
and the Files list and the diff title count reviewed hunks, with `◐` marking the
partly reviewed file. Press `l` or click the folded row to read the hunk again, and
click its `☑` (or press `rh` on it) to mark it unreviewed. When an edit later
touches a reviewed hunk, it reopens with only the change since you reviewed it,
labelled "changed since review"; reviewed hunks the edit did not touch stay
folded. Accepting the last open hunk marks the whole file reviewed and moves to
the next file. Binary, conflicted and symbolic-link changes are reviewed per file.
Older reviewer builds show partly reviewed files as unreviewed.

Press `rf` in Files or its diff pane to automatically mark files whose changes
Jev classifies entirely as insignificant. In the other files it marks each hunk
whose changed lines are all insignificant, as `rh` would. A hunk that rewrites
lines you already approved stays open, because Jev judges changes against the
base, and a file with a mode or type change keeps at least one hunk open, since
only marking the whole file covers that change. This uses the same classification policy
as Explore and requires `TYPESAFE_API_KEY` in the reviewer process. It checks each
unreviewed file's full change in the current comparison, including files changed
since an earlier review. Classification runs in the background; a notification
reports the result. Significant, uncertain, failed, oversized, unclassified, and
metadata changes remain for review. Changing the comparison or manually changing
review marks cancels the run. Automatic marks use the classified checkpoint, so
later edits need review again. `Space` can undo an automatic mark.

Press `rU` (lowercase `r`, then uppercase `U`) to set all files in the current
change to unreviewed. The confirmation dialog accepts `y` to confirm and `n` or
`Esc` to cancel. Confirming cancels any running Jev autoreview and clears file
review marks; discussions and other changes' review marks are retained.

Review comments use the last focused agent in the Herdr workspace. Focus
another agent to switch; pending comments follow that selection, while
completed answers and conversation history remain.

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

File badges show 💬 for unresolved conversations. A red `●`
marks unread replies beside the filename and in the Threads tab, including late
replies on resolved threads. A reply is marked read once every part has been
visible, including when you scroll through a reply taller than the pane.

For connection problems or delayed notifications, see
[Agent connection through MCP](mcp.md).

## Explore a change (experimental)

Open **Explore** and click **Start** or press `s`: the round starts, and its
[Explore page](#start-follow-and-answer-an-explore-round-in-the-browser) opens in the default
browser of this machine. The round then runs on the page: the Explore tab shows no question,
answer or conclusion, only that the round runs on the Explore page, where it stands in one
line (starting, the agent is working, a question waits, concluded, or interrupted and why),
**Open the Explore page** (or `o`) to open it again, **Reset**, and the page's address and QR
code. A round started on the page shows the same way. **Continue in the pane** (or `i`) shows
the interview in the pane instead, for when no browser is at hand; it is the same round, and the reviewer
remembers the choice when you reopen it. When the browser cannot open the page (no browser,
no display), the round still starts, and the pane says why and gives the page's address to
open by hand.

**Start in the pane** (or `p`) starts the round in the pane, as Start did before the page
existed: the pane shows the interview and opens nothing. The footer lists the keys of the four
buttons while the start screen shows them. Explore prepares the complete
base-to-working-copy change, including reviewed and filtered files. The selected implementation agent investigates the change with you in its existing
conversation, asking at most one next question per turn. Requests are sent through
Herdr immediately, including while the agent is working, using the same delivery
as thread comments. Each delivery uses the selected agent; an in-flight attempt
remains bound to that selection until it resolves.

**Start with Challenger** (or `S`), and **Start in the pane with Challenger** (or `P`)
without the page, start the same round with a second point of view. The
agent starts a subagent with fresh context, the challenger, which reads the same prompts and
the same diffs without knowing why the change was written the way it was. Each turn the two
each propose a question; they take turns having theirs asked, the challenger first, and the
other's waits and is judged again after your answer. The agent that did not propose the
question gives its facts and position on it, and the question you see is one text written
from both. You still answer one question per turn and nothing in the pane changes, except
that turns take longer. The round ends when neither has a question left. The choice holds
for the round.

The agent writes every text you read in the round (the design, the questions with their
context, choices, door and blast radius, its replies, the conclusion and the quiz) in the
[writing style](#explore-round-settings) the round started with: Simplified Technical English
by default, or plain. Whatever the style, when the design or a question presents a
behavior change, the agent says what changes, then why (the problem, what goes wrong without the
change, and where that reason is stated, or that it infers it), and only then how.

When every changed line is marked as reviewed, a round has nothing to ask: the four start
buttons stay on the start screen, inactive, with a line that says nothing is left to review,
and their keys do nothing. Unmark a line or a file, in Files or in the diff, and they are
active again, without reopening the reviewer. A start that finds nothing left to review once
it began, because Jev or another reviewer marked the rest meanwhile, sends no kickoff and
says why. A round that already runs goes on when its last line is marked.

**Reset**, in the top-right corner of the Explore pane, closes the round and returns to the
start screen, where you choose how to start the next round. Click it, then click
**Confirm reset** within five seconds; any other action cancels it. The closed round stays
saved but is no longer shown, and its agent can no longer post to it. Reopening the reviewer
after a reset shows the start screen. The next round starts over: its kickoff lists none of the
reset rounds' decisions and tells the agent that their questions, answers, decisions, tasks and
question numbers are void, even when the agent's conversation still holds them. The review
marks that the reset rounds applied stay; unmark files to have them asked about again.

The kickoff prompt supplies the review target, the change description (jj) and the interview
instructions, which ground every question in what the change does. The agent
reads files directly, uses Git/jj for the full diff, and posts the first question
with `submit_question`. Each human contribution sends a wakeup containing the rules for that turn and the exact
selected option's full text and optional comment, plus IDs identifying the original
question and turn. The agent uses its existing conversation and posts the next turn
with `submit_question`, or concludes with `submit_conclusion`; evidence and assessments are not sent back to it.
The agent normally keeps context in its conversation. Wakeups do not include the
full history, so a newly selected conversation may need the reviewer to supply
missing context. No history dump, repository catalog, mailbox or file fallback
is exchanged. Refresh the agent's MCP connection after upgrading to update its tool catalog.

**Keep code unchanged while continuing a round, including after reopening.** Explore reads working-copy files
directly and assumes they stay unchanged. Saved decisions describe that investigation; they do not establish that later edits were reviewed. Reset and start a new round when code has changed. It does not snapshot the repository or
pause decisions when source files change. Supporting files are read as needed,
so large unchanged assets do not impose a repository-wide capture limit.

**Explore progress is saved automatically.** Opening the same checkout and logical
review restores its latest round: exact questions, answers, agenda, conclusions, the
review marks each answer led to, separate task/reply drafts, choice selection and reading
position. Rounds saved by earlier versions, before answers could mark lines, are not restored.
Reopening sends no prompt and never starts implementation. The next explicit
action prompts the selected implementation agent. If that agent is unavailable,
history and edits remain available until it can be selected and retried. A new
native conversation alone does not require a new round.

Accepted responses, posted answers and implementation authorization are saved before
acknowledgement or delivery. Editor changes save in the background and flush on a
normal close; an abrupt process death can lose keystrokes still awaiting a save.
Write errors stop unsafe Explore changes. Files
and Threads remain usable. Explore supports one agent and one reviewer per repository,
with one saved editor and reading state per round.

Explore shows one question at a time across the full content width. Its evidence,
answers and recap scroll together. **Previous** and **Next** (or `[` / `]`) visit
question and conclusion history in posting order; **Latest** returns to the newest question or active conclusion. These controls stay
visible while scrolling. **Conclusion** opens the separate conclusion page.
Concept exploration determines when the discussion ends; the agent does not keep
asking only to mark more lines. What is left to review shows where it always does:
the header's review progress and the Files list. Once the agent has exhausted its
concept agenda, it checks the unreviewed lines for missed questions, then concludes;
lines no answer settled stay unreviewed.
Each question keeps its own unfinished text, selected choice and evidence state.

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
about the conclusion, without authorizing code changes. Explore does not resolve
ordinary threads.

After each answer, the agent marks the changed lines your answer settled as reviewed,
and can reopen reviewed lines your answer made matter again, whoever marked them. These
are ordinary review marks: Files, the diff and the header progress show them, and you
can reopen or mark hunks yourself as usual. On any turn, the first included, the agent can
also mark lines it read and found **not relevant**: lines that hold no decision for you,
such as removed code the change is about removing, mechanics that tests cover, and tests,
docs and manifests that follow the code. Each mark gives one of these reasons, and a mark
for mechanics that tests cover names the test, its file and lines, so you can check it
in a few seconds: the list of marked lines shows them, as in
`src/parse.rs new 10-24 (not relevant: mechanics covered by tests, see tests/parse.rs 5-30)`.
Reopen them in Files if you disagree.

Review progress moves when you act. The marks that come with a question wait until you
answer it: until then the end of the question's page, after the evidence, says what it
"Will mark … when you answer", expandable to the lines. Once you answer, they are applied
and the recap moves below your answer. So the lines an answer settled are marked when you
answer the next question, or at once when the agent concludes instead. Cancelling an
answer gives back everything it applied. Marks apply only while the code is still the
checkpoint the round started from. Every prompt points the agent at its remaining
**Unreviewed lines**, the changed lines no review mark covers, including your own marks
from Files: a private temporary directory holds them as one diff per file, named like the
file, each row with its old and new line number, so the agent reads what is left without working it out from
the full change. Every prompt gets a new directory, and the previous one is removed. If
the diffs cannot be written, or the repository cannot be read, the prompt is not sent and
a toast says why; Retry tries again. When Jev is enabled, starting a round first marks what Jev dismisses,
as `rf` does; the agent only sees what is left.
The preparation state shows the actual pending status and **Stop waiting**. Delivery
errors and Retry appear beside the affected turn. When the agent does not start on
a prompt within a few seconds, the turn says so: its text may still wait in the
agent's prompt box, so look at the agent's pane, then Retry, which sends the same
turn again. An implementation request the agent did not start on offers Retry too.

On follow-up turns, the agent's reply appears above the question. The question is
followed by its choices and the text field for optional details.
The **Context**, **Door** and **Blast radius** Markdown sections follow, then
**Notes** and the evidence. Each starts with a short summary paragraph,
followed by useful detail. Context explains the behavior and unfamiliar terms,
with depth adapted to what you already know. The provisional map can be
expanded alongside a question. Every question
offers two to five alternatives plus **None of the above**, with the first selected by default.
Use `Up` / `Down` or `j` / `k`, click a choice, or press its number to select it.
The text field adds optional details to the selected choice; changing the choice keeps
those details. Click **Send**, press `Ctrl-Enter`, or press `Enter` while selecting choices
to submit both together. **None of the above** keeps the inquiry open and can be sent
with or without text. Click the text field or use `Tab` to reach it. The text fields
use the shared comment editor: `Esc` returns to Normal mode in Vim editing and stays
in the field in Regular editing. `Tab` returns to Explore commands. The short recap
shows the agent's interpretation. Ask “Why?”, request a caller,
challenge an assumption, or provide new context in the same free-text composer.
The agent replies directly; a factual question or context is not agreement. Conditional decisions retain
their required changes as follow-ups. Free-text interpretation remains something
to check, and to amend with another answer; it is not a semantic guarantee.

**Cancel answer**, under your latest answer, takes it back, also while the agent is
still working on it. The agent's turn after it is discarded, the review marks that
turn made are given back (lines it marked reopen; lines it reopened return to whoever
had marked them), and the question comes back with your choice and text ready to
change and send again. Repeat it to take back earlier answers one at a time. The next
prompt tells the agent which answers were cancelled. Answers cannot be cancelled once
**Implement** was sent. When the code changed since the round started, lines the
cancelled turn reopened stay open.

| Explore command | Action |
| --- | --- |
| `Up` / `Down` or `j` / `k` | Select an answer, including None of the above |
| `Enter` | Submit the selected answer and optional details when outside the editor |
| `[` / `]` | Previous / next question |
| `e` / `b` | Next evidence / first evidence |
| `m` | Expand map and follow-ups |
| `PageUp` / `PageDown` | Scroll the conversation when it has focus |
| Mouse wheel | Scroll code over a diff; scroll the conversation outside it |
| `Alt-j` / `Alt-k` | Grow / shrink the selected evidence window |
| `Alt-0` | Fit evidence automatically again |
| `c` / `r` | Stop waiting for pending work / explicitly retry |
| `s` / `S` | Start a round and open its Explore page / with a Challenger, from the start screen |
| `p` / `P` | Start a round in the pane only / with a Challenger, from the start screen |
| `o` / `i` | Open the Explore page again / continue in the pane, during a round on the page |
| `Tab` | Cycle conversation, evidence and answer focus |

Each accepted new question opens automatically, including after input while waiting.
Any unposted text stays with its original question and is restored through history.
If you are in Files or Threads, Explore selects the new question for your return
without switching panes. Switching Files/Threads/Explore preserves questions, drafts
and code position. Narrow terminals reflow the question and history controls without
changing Files' sidebar.

The initial native diff window fits the selected evidence's wrapped rows plus
context, capped at about half the content height. Larger sources remain fully
scrollable. Each snippet explains what it establishes and how that could change
your answer. **Evidence** lists the question's citations, most decisive first,
followed by those its assessments, the agent's reply and its agenda changes cite. Every cited range in
the shown file is outlined. Repeated citations of the same range appear once.
Drag the bottom edge or use the resize keys; **Fit evidence** restores automatic
sizing and positions the complete range, including its outline and wrapped lines. Manual
heights and each opened viewer's position, search, selection and comment draft
stay independent. Overlapping ranges share an outline and fit together. Distant
ranges remain separate rather than enlarging the window to include unrelated
code. Non-text or unavailable sources show a compact limitation.

Yellow outlines mean **relevant to this question**. They do not mean accepted,
reviewed or high risk. Each code window retains search, selection, comments, syntax
highlighting and new-side language-server navigation. Press `b` to return to the first
evidence after following a definition. Old/deleted-line LSP operations are unavailable;
additional regular working-copy files inside the repository can be opened on demand.
Citations name paths, sides and lines directly. Outside-repository destinations
and non-regular files remain unavailable.
Old-side references open at their old coordinates. A range outside displayed diff
hunks opens the available full base text in the native viewer. It retains historical
coordinates; it is never substituted with current working-copy text.

A reset round stays in storage; a new round does not inherit its decisions (see Reset above). Invalid responses leave the last usable question and answer
available. MCP validation errors let the agent repair the same pending turn;
an identical retry of an accepted result is acknowledged without replaying it.
Responses are bounded to 1 MiB; exceeding that bound is
reported without truncation. Stored rounds can grow across many responses (up to
256 MiB per round; editor records up to 16 MiB). Corrupt, oversized or unsupported
records are cleared with an error toast when possible; readable history is
retained. Request and answer identities still protect against
cancelled, duplicate or unrelated responses; they do not establish source freshness.

Consequential questions show separate **Door** and **Blast radius** sections.
The first assesses whether effects can actually be undone, including rollback or
rebuild conditions; the second describes plausible harm, propagation and bounds.
Additional reasoning and unknowns appear below their summaries without an expansion button.
Their citations open in the same native viewer.
These are evidence-backed agent judgments, not risk scores or guaranteed safety.

The agenda is provisional. Context can add, refine, reorder, retire or supersede
pending inquiries. Retirement keeps the reason and original wording and does not
mean acceptance. A reconsideration flags a prior conclusion without changing the
original decision; conditions remain outstanding. New questions
do not imply a fixed total. The reviewer saves conversation and agenda history; the agent retains context in its own conversation.

The expanded map shows those states, prerequisites, entries not yet mapped and scan limitations. Topic
associations do not mark lines reviewed. A successful conclusion saves the discussion outcome and can mark the lines the final
answer settled. Explore does not resolve threads.

Setting a nonempty `TYPESAFE_API_KEY` in the reviewer process enables Jev
significance checks: `rf` in Files, and the start of each Explore round, mark the
changed lines Jev judges insignificant as reviewed. The reviewer sends bounded
before/after code snippets and relative paths to TypeSafe AI. Missing or
whitespace-only keys disable Jev. Uncertain, failed and oversized checks leave their
lines unreviewed. Reopen a hunk in Files to review Jev's lines yourself.

## Start, follow and answer an Explore round in the browser

The **Explore page** shows the open reviewer's Explore round in a browser, and you can
run the whole round there, recoveries included, without going to the pane: start it, answer
the agent's questions, talk with the agent in a chat, stop waiting, retry, cancel an answer,
implement the conclusion, and reset the round. Run the Herdr
action **Open the Explore page of the progressive reviewer**
(`herdr.progressive-reviewer.explore-page`) from the workspace of an open reviewer: it
opens the page in the default browser of the machine that runs the reviewer. Set
`BROWSER` in Herdr's environment to choose the program; otherwise the action uses
`xdg-open` on Linux and `open` on macOS. **Start** and **Start with Challenger** in the
pane open the same page, with the same program, once the reviewer has captured the change,
and so does **Open the Explore page** during a round on the page; Retry, Reset, a round
started on the page and the pane's other buttons open nothing. To run
the action with a key, add a shortcut as in the [README](../README.md#get-started):

```toml
[[keys.command]]
key = "prefix+e"
type = "plugin_action"
command = "herdr.progressive-reviewer.explore-page"
description = "open the Explore page"
```

If you always work in the pane, turn **Open the page on Start** off in the
[Explore page settings](#explore-page-settings) (`w`): Start and Start
with Challenger then start the round in the pane, as Start in the pane does. On, the default,
opens the page. The action opens the page whatever this setting says.

The page shows the round's current stage: the design of the change, a question with its
explanation and choices, that the agent is working, that it is no longer working on its turn
and why, the quiz, the conclusion, or that no round is running. Above a question, the quiz or
the conclusion, it shows your previous answer ("You answered Q1") beside what the agent said
back to it, as the pane does: its recap of the answer ("The agent recorded"), its reply, and
the follow-ups it recorded. While no round is running, the page
names the review it belongs to, as the pane's header does (the change's title and
revision), its repository and the size of its change, so that the pages of two reviewers can
be told apart.

When no round is running, the page offers **Start** and **Start with Challenger**, as the
reviewer's Explore tab does. The round starts as it does from the pane: the reviewer
captures the change, Jev first marks what it dismisses when it is enabled, and the agent
gets the kickoff. Meanwhile the page says that it prepares the round, then that the agent
is working, then opens the round on the design of the change, which leads to the agent's
first question. The Explore tab shows the same round:
while it starts, the tab says so and offers **Stop waiting**, which drops it.
When the reviewer cannot capture the change, the page says why and offers Start again. A
round started in the reviewer or in another tab after the page was loaded wins: the page
refuses to start a second one, says so, and shows the round as it is now. When nothing is
left to review, the page's Start and Start with Challenger are inactive and a card says why
("Every changed line is reviewed"), as in the Explore tab; they are active again as soon as
you unmark a line, with no reload.

To answer a question, pick a choice or None of the above, write an optional comment, and
press **Send answer**; a comment without a choice works too. The choice the agent
recommends carries a purple tag, with its reason. A question whose Door is one-way, mixed or
unknown (its chip says so, beside "Blind pick") first hides the recommendation: the page
lists the agent's choices in a mixed order, the same each time it shows the question, with
None of the above last and none selected. Pick one, write your comment already if you like,
and press **Send answer**: this first Send sends nothing yet. The page shows the
recommendation, with a line that says whether you and the agent picked the same choice, your
first pick tagged and selected, and your comment still in its box; you keep or change them,
then press **Confirm answer**. The comment is not part of an answer until you confirm it. The
reviewer saves your first pick with the answer, for statistics; the agent receives only the answer you send. A two-way question,
or one with no Door, shows the recommendation at once, in the agent's order. So does a
question asked again after a Cancel answer: you have seen its recommendation, and the
answer keeps no new first pick. The agent receives the answer as
if you had given it in the reviewer's Explore tab, which shows it under the question.
Above **Send answer**, the page says how many lines your answer marks reviewed (the lines the
agent found not relevant count with them, as in the meter), and the share of the change that is
reviewed before and after it; open that line to list the lines, each with how it is marked.

Once you send, the page says that the agent is working on your answer to that question, with
the time since the answer went out ("Sent 0:42 ago"), and under that the question you
answered, read only, as you read it before you answered, its citations open or folded as you
left them. Beside it (above it on a phone), the panel shows the question's choices with the one
you sent selected, your comment, what the answer marked, **Stop waiting**, and **Cancel this answer…**. When the
turn does not go through, the panel offers **Retry** instead of Stop waiting. The page shows
the agent's next question or its conclusion as soon as the agent posts it.

The page offers the actions of the reviewer's Explore tab for the round's state, and they
save the same result and send the agent the same prompt as there:

- **Stop waiting**, while a round starts or the agent works: a round that is starting is
  dropped; for the agent's turn, the page then says the agent is not working on it and
  offers Retry. Use it when the agent never starts on a prompt that the reviewer sent.
- **Retry**, when the agent is not working on the turn the round waits for: its prompt
  could not be delivered or the agent did not start on it (the page says why), you stopped
  waiting, or the reviewer was
  reopened during the turn (when the prompt may have reached the agent already, the page
  says so: check the agent's pane first). Retry sends the same turn again, the
  kickoff included.
- **Cancel this answer…**, under your previous answer above the question, the quiz or the
  conclusion, and in the panel of the answer you sent while the agent works on it or its turn
  waits for Retry, behind **Confirm: cancel my answer**: the round goes back
  to the question that answer answered, with the agent's turn after it and the review marks
  it led to taken back. It is offered until an implementation request is made.
- **Reset**, in the page's ⋯ menu at the top right ("Reset this round…"), behind
  **Confirm reset**: it closes the round for good and the page offers Start again. The
  round's records stay saved. Once the agent received the implementation request, **Start a
  new round…** in the conclusion's panel does the same, behind **Confirm: start a new
  round**. The page offers Reset nowhere else.

To ask the agent something, challenge it or add context without answering, write in the chat:
the round's conversation with the agent, which the page offers on every screen of a round.
Open it with the speech bubble at the top left (at the top right on a phone), with **Open the
agent's conversation** in the ⋯ menu, with **Not ready? Reply to the agent instead** under the
conclusion's list (**Reply to the agent** once the agent received the implementation
request), or by selecting a passage of what you read (the question, its explanation, its
citations, the design, the conclusion) and choosing **Add to chat**, which quotes it in your
message. Write, then press **Send** or
`Ctrl-Enter` (`⌘↵` on a Mac). Your message names where you wrote it (the design, a question,
an earlier question or the conclusion) and wakes the agent as a comment in a thread does: it
answers nothing, the question stays open, and the round does not move. The chat says that the
agent is answering, with the time since, and the agent's reply lands in the chat, not above
the question. A reply you have not seen counts on the bubble and in the browser tab's title
("(1) Q1 · your turn") until you open the chat. When a message did not reach the agent, the
chat says why and offers **Retry**, which wakes the agent again for your waiting messages. On
a desktop the chat is a column at the left of the window, under its bubble: on a window wide
enough (1424 pixels and more), what you read moves over to make room, so you read the
question, see its choices and talk to the agent at once; on a narrower window it lies over
the left of the page and leaves the choices in view. On a phone it is a sheet over the page,
which you drag up to the full height, or down to close it. `Esc` or × closes it.

The row at the top of the page names the review and shows where the round stands: the
design, each question asked so far, the quiz (the item it shows while you take it, then its
score), and the conclusion;
done steps carry a green check, the current one is underlined and says when the agent works
on it ("Q2 · working"). Each done question opens that question again, read only (see below).
**Design ▾** opens a map of the design: its thesis and its four
parts, each a link into the design, and **Open the design →**. The ⋯ menu also copies the
round's link and opens the chat. The
browser tab's title says whose turn it is ("Q3 · your turn", "Agent working…", "Retry
needed", "Conclusion"), after the number of chat replies you have not seen, so you can leave
the tab while the agent works. On a phone the row shows the design and the current step, and the
steps of the screens beside the one you read when there is room for them; the review's title
is in the menu.

The line under the row is the meter: its green part is the share of the change's changed
lines that review marks cover. Point at it, or focus it, and it grows and splits by who
marked the lines; a window gives the numbers: the share reviewed, the lines marked out of the
changed lines, the size of the change, the lines your answers marked (with the questions),
those marked by hand or in earlier rounds, by Jev and as not relevant, those the question that
waits marks when you answer it, and those left to explore, then a row for each file with its
lines left, "cited here" beside the files the current question cites. A click, or `Enter` on
the focused meter, pins the window open; `Esc` or a click elsewhere closes it. When an answer
applies its marks, the green part grows.

An action on a state that changed meanwhile, in the reviewer or in another tab, is refused:
the page says which action did nothing and why, and shows the round as it is now. An
answer is recorded once: when the question already has an answer, the page refuses to send
yours. The pane shows the result of each action taken on the page. The page follows the
round in every state, with no reload: it keeps a connection to the reviewer, which sends each
change as it happens, and the page changes in place only what changed, so the text you are
typing, where you scrolled to, the focus and what you opened stay as they are. What you type
in a text box (a comment, the list to be implemented, a chat message) is kept for as long as
the tab stays open: the box shows it again whenever the page shows that same question (the
same version of it), conclusion or chat again, after a reload too; the next question starts
with an empty box. While the page cannot reach the reviewer (the reviewer restarts, the
network drops), a line at the top says that it reconnects, and your actions wait, their
buttons inactive; once the reviewer is back, the page shows the round as it is now.

A round that can no longer change, an earlier round or one whose saved history had to be
repaired, says so and offers only Reset. When the reviewer cannot save Explore rounds, the
page says why and that nothing can be done until the problem is fixed and the review pane
is opened again.

When the agent's conclusion carries a quiz, the page first asks its few questions about
how the system works after the change, one at a time ("Quiz · Question 2 of 3"): pick an
answer and press **Check**, and the page says at once whether it is correct and why, marks
the correct answer and your pick, and shows the lines that prove it. **Next question** goes on
(**Show the conclusion** after the last one), and **Skip the quiz** goes straight to the
conclusion, the questions left counted as skipped. The quiz marks no lines. The picks are
saved with the round. The conclusion's panel then opens with the score ("Quiz 1 of 3"), and
its **See the answers** opens the results, question by question, beside the conclusion. The
pane shows no quiz.

The conclusion shows the agent's summary, then **Your decisions**: each question you
answered, with the choice you kept, tagged "as recommended" or "changed after your first
pick" when it was, then the rest of the summary and the future work. The panel holds the list
to be implemented, whole: plain text that you can edit before you press **Implement**
(**Implement 3 items**, which counts the lines that are not blank), which
authorizes the agent to implement that list and nothing else, as **Implement** in the
reviewer does: the agent receives the same request, and an empty list is refused. The
page then says that the request is being sent, then that the agent received it and when, with
the list it sent, and offers **Start a new round…** (Reset, above) and **Reply to the agent**,
which opens the chat. A conclusion gets one request: once one was sent, from the reviewer or
from another tab, the page refuses another and shows the request that was sent. When
the request could not be sent, the page says why and offers **Implement** again with your
list. While the request is being sent, **Cancel the implementation request** stops it if it
has not reached the agent yet. A request saved before the reviewer was reopened, and never
sent, can be sent as it was saved, with **Send the saved request**, or edited under **Edit
before sending** and sent with **Send a new request**; for a request
whose delivery is unknown, the page asks you to check the agent's pane before you
send a new one, with **Send a new request anyway**. When the agent did not start on the request, its list may still wait in the
agent's prompt box: the page offers only **Retry**, which sends the same request again.

The round opens with the design of the change, which the agent explains before its first
question: one sentence for the whole change, its thesis, then four parts, each opening with
its own one-sentence thesis: what the change adds and where, its types and data flow, its
algorithm and cost, and the alternatives the implementer rejected. The page shows it as a
screen of its own, the first one of the round: the change's thesis as the headline, with the
number of parts, about how long they take to read and how many files the change touches,
then the four parts, each led by its thesis. Beside them (under the thesis on a phone), the
design map links to each part and marks the one you read, then names the question the round
waits for, with **Go to question 1**, which goes on to it; on a phone that button sits in a
bar at the bottom of the screen. The round opens on its design once per tab: a reload after
you went on shows the question, and when the round moves on in the pane while the page shows
the design it opened, the page shows the round's new stage. Later, **Design ▾** on the row at
the top opens the map; a part, or **Open the design →**, opens the design screen again, where
**Go to question 2** (or **Go to the quiz**, **Go to the conclusion**) returns to where the
round stands, and the browser's Back returns to the screen before. The pane
shows it before question 1, the change's thesis under "Design of the change" and each part's
thesis in bold before its text. A round saved before theses existed shows the first
paragraph of "What it adds and where" as the change's thesis, and the first paragraph of each
part (the next one for "What it adds and where") as that part's.

Each question you answered is a done step on the row at the top ("✓ Q1"). Click it to read
the question again as you answered it: its explanation, Door and Blast radius and citations,
and beside them its choices with the one you kept selected, your comment and tags, what the
answer marked, and what the agent recorded, then
**Go to question 3** back to where the round stands. Nothing on this screen can change the
round. On a phone, swipe sideways to turn between the design, the questions you answered and
the round's current stage, in the order of the row at the top: drag far enough, or flick, and
the page turns, the chip of the screen it turns to filling as the drag gets there; let go
before and the page springs back. A drag that starts in a table, code or a diagram that
scrolls sideways scrolls it instead.

A question's explanation is its Context, which the agent writes in Markdown: short
paragraphs, lists, code, and tables whose cells can carry a good, bad or warning mark, and
callouts for a conclusion, a tip, a warning or an error. The **Door** and **Blast radius**
sections are folded to one line, their decisive reason, until you open them. The page shows
raw HTML in the agent's text as
text. In the pane, a callout opens with its title and a marked cell with its mark (✓, ✗ or
!).

After a question's explanation (and after its choices on a phone), the page shows the
question's citations in the order the agent gave them, most decisive first: each with the agent's note and the cited lines as rows of the
diff, with their line numbers and syntax colors. The other citations stay folded under the
first one until you open them. A citation of a whole file, or of a file that is not text,
says so instead of showing lines. So does a citation of a file that is neither part of the
change nor tracked by the repository, such as an ignored `.env`: the page can be open
from the network, so it shows no other file of the working copy. The pane shows such a
file's lines. On a narrow screen, scroll a long line sideways. Reading
a citation on the page marks none of its lines reviewed.

The agent may also draw a diagram, as a fenced `mermaid` block: the page draws it with Mermaid,
which the reviewer serves itself, so the page needs no internet access. A wide diagram shrinks to
show whole, widening to the whole reading column first when its text would get too small; on a
phone, one that would be too small to read keeps its size and scrolls sideways, and says so.
**Open large** shows a shrunk or scrolling diagram at its full size over the page (Escape
closes it). A flowchart drawn left to right that does not fit is drawn top to bottom instead. When Mermaid cannot read a diagram, the page says that it could not be
drawn and shows its source, with Mermaid's message behind a fold, and the reviewer saves the
error with the question. The pane shows a diagram
as its source.

The action opens the page on this machine (`127.0.0.1`), on a port chosen when the reviewer
starts. Its address carries a token that changes each time the reviewer starts: the page
refuses a request without it, and a request from another site. Run the action again
after reopening the reviewer. Closing the reviewer stops the page.

The page lays itself out for the width of the browser. On a phone it is one column, and a
wide table, line of code or diagram scrolls sideways in its own frame. A wider window gives
the explanation, its tables, the cited code and the diagrams more room, so that those that
fit the window show whole, while paragraphs keep a readable line length. From about 1120
pixels wide, a question reads in two columns: its explanation and citations on the left,
and its choices, comment and **Send answer** on the right, which stay in view while you scroll
through the explanation; a conclusion keeps its list and **Implement** on the right the same
way, and so do the design with its map, an answered question with your answer, and the quiz
with its answers.

When you come back to the page (a phone that wakes, a tab shown again, the network back), it
checks at once that its connection to the reviewer still works, and opens a new one when it
does not; a new connection brings the round as it is now.

### Open the page from a phone

The end of the pane's Explore tab, on every screen of it, the start screen included, shows
the address of the page on the network and its QR code; a pane too narrow for the code shows
the address alone. Scan it with a phone or a tablet on the same
network to start a round there, or to follow the running round. A page whose round was
reset or replaced while the phone slept says that its address no longer opens a round:
scan the new code. The reviewer serves this page on a second listener, on the address of
one network interface, over plain HTTP. Each round gets a new token. While no round is
running, the start screen has a token of its own, and the round started next keeps it, so
the phone that started the round stays on it. After a reset, the round's address is refused
and the pane shows the address of the next round's page; the page that sent the Reset moves
to that address itself, so it can start the next round. A round started while another runs
gets a new address too. The page answers only the address the pane shows, and the
MCP endpoint is never served on the network.

Anyone who can read your network's traffic can copy the token and use the page as you until
the round ends, and on the start screen before it; a Reset from such a copy hands it the
next start screen's token too ([ADR 0003](adr/0003-serve-the-explore-page-on-the-network.md)).
Turn network access off where you do not trust the network, in the
[Explore page settings](#explore-page-settings).

A reviewer that cannot serve the page on the network (no network address, an unknown
interface, all ten ports taken) keeps the page on this machine, and the pane says, in one
dim line where the address and the QR code would be, that the page is not shared on the
network and why.

A firewall that drops incoming connections blocks the phone. With ufw, allow the ten ports
from your local network, for example:

```sh
sudo ufw allow from 192.168.1.0/24 to any port 8790:8799 proto tcp
```

Remove the rule with `sudo ufw delete allow from 192.168.1.0/24 to any port 8790:8799 proto tcp`.

### Explore round settings

Every screen of the Explore tab also shows the settings for the rounds, above the Explore
page settings. They are saved with the reviewer's other settings, as the page settings are. A
round keeps the writing style it started with: a change applies to the next round, not to a
round that runs. Run-ahead applies to the question that waits at the reviewer's next action:
turning it off then stops the forks that run.

| Setting | Key | Effect |
| --- | --- | --- |
| **Writing style of the next round** | `W` | Simplified Technical English, the default: the prompts ask the agent to apply the writing rules of ASD-STE100 Simplified Technical English, without its controlled dictionary, to every text you read (short full sentences, one topic each, in the active voice, the ordinary technical words of the code's domain allowed, technical names kept as they are in the code), on every turn of the round, the conclusion and the quiz included. Plain: the agent writes in its own style, as before this setting existed. The style changes the wording and the length, not what the agent tells you. |
| **Run ahead** | `z` | Off, the default. The recommended choice, or every choice: while a question waits for you, the reviewer forks the session of the agent in the pane once per such choice, as background `claude` processes, once the agent is idle. Each fork takes the turn that would follow that answer, on the prompt the agent would get, and the reviewer keeps what it submits. When you answer with a choice whose fork submitted its turn, with no comment, and nothing changed since the forks were taken (no message in the round's conversation, the agent idle with an empty input box and on the same session, the same unreviewed lines), the agent in the pane resumes the fork's session (`/resume`), the fork's turn becomes the round's, and its question shows within a second or so, with the line "Prepared while you were thinking" in the pane and on the page. Any other answer goes to the agent in the pane, as with run-ahead off. If the agent cannot switch, the turn waits for Retry, which sends your answer to the agent in the pane. The other forks are stopped and their transcripts deleted when you answer, cancel an answer, reset, or close the reviewer. Each fork costs about one agent turn (it reads the agent's prompt cache). It needs Claude Code in the agent's pane, started with options run-ahead knows and without a `--settings` of its own (a fork brings its own). What the forks do is written to `run-ahead.log` in the plugin's state directory. |

Click a setting, or press its key while you are not typing an answer, to change it. Rounds saved
before the writing style existed continue in the plain style.

### Explore page settings

Every screen of the Explore tab shows the settings of the Explore page with their values, at
its end, above the page's address and QR code. No setting needs the reviewer to be reopened:
a change takes effect at once, and is saved with the reviewer's other settings
(`settings.json` in `$HERDR_PLUGIN_STATE_DIR`), which the reviewers of the machine share. A
change saves only the setting it changes, so it keeps what another reviewer saved, and the pane
then shows the settings as saved. Another reviewer that is already open does not see a change
at once: it keeps its own settings, and network access as it was, until it is reopened or
saves a change of its own; then it applies the saved settings, including what this reviewer
changed. Turn network access off in each open reviewer, or reopen them.

| Setting | Key | Effect |
| --- | --- | --- |
| **Open the page on Start** | `w` | On, the default: Start and Start with Challenger open the round's page in the browser. Off: they start the round in the pane. |
| **Serve on the network** | `n` | On, the default: the page is also served to the network, and the pane shows its address and QR code. Off: the page stays on this machine; the network listener stops, a phone's page stops working, and the address and QR code leave the pane. |
| **Interface** | `N` | The interface whose IPv4 address the page listens on, such as `wlan0` or a VPN's `tailscale0`. Empty, the default (shown as "default route"): the interface of the route to the internet. |
| **First port** | `#` | The first port tried, 8790 by default. When another reviewer holds it, the page takes the next free one of the ten ports from it. |

Click a setting, or press its key while you are not typing an answer, to change it. The two
switches turn over at once. Interface and First port open a one-line editor under the
buttons, with the current value: `Enter` saves it, `Tab` or any other button leaves it
unchanged, and a first port that is not a number from 1 to 65535 is refused with the reason. A new interface or port moves the page to a new address,
and the pane shows the new QR code; scan it again.

## Explore statistics

To see whether a change to Explore helps (a prompt change, the Challenger, a new page),
print the numbers of the saved Explore rounds from inside the repository:

```sh
/path/to/herdr-progressive-reviewer/bin/reviewer-control stats
/path/to/herdr-progressive-reviewer/bin/reviewer-control stats --since 2026-09-15 --until 2026-09-30
```

The command reads the rounds saved for the checkout it runs in; each jj workspace keeps
its own rounds. It only reads them and changes nothing. It reads Herdr's state directory
for the plugin, `$XDG_STATE_HOME/herdr/plugins/herdr.progressive-reviewer` (by default
under `~/.local/state`), or `HERDR_PLUGIN_STATE_DIR` when set.

It prints one table for all rounds and, when you give `--since` or `--until`, a second one
for the rounds started in that period. A date covers that whole day in local time; an
RFC 3339 time such as `2026-09-15T14:00:00+02:00` is exact, and `--until` excludes it.
Each table has a column for all rounds, one for rounds with a Challenger and one for
rounds without. The rows are:

- the number of rounds, and how many have answers and a conclusion;
- the questions per round (median and range), counting only rounds with an answer;
- the share of answers that asked for a change, which the agent interpreted as needing a
  follow-up, out of the answers its next turn took up; an answer it left uninterpreted,
  as it may for free text, asked for nothing;
- the share of answers that did not choose the recommended choice, out of the answers to
  questions that recommended one; an answer in free text alone did not choose it;
- the median time of an agent turn, over every turn and over the turns after an answer;
- the median time the reviewer took to answer, from the agent's question to the answer;
- the median share of a round's time, agent turns plus answers, spent waiting for the agent;
- the median number of words the agent wrote for a question: its text, context, sketch,
  choices with their recommendations, evidence notes, assessments, and the reply above it;
- over the rounds with a Challenger, the number of questions the Challenger proposed, the
  number of rounds that reported any, and how many were asked, merged with the agent's own
  question, retired by a fact or kept for a later turn. A proposal that several turns
  report, by the same title, counts once, at the result of the latest turn. A round saved
  before the agent reported proposals has none, so it is not among the rounds that
  reported proposals.

A round with no answer counts as a round and stays out of the numbers about answers. The
command also says how many saved rounds it could not read: rounds saved by earlier
versions and damaged records. Rounds whose turns carry no time, saved before turns were
timed, count in the period by the time their file was last saved. A retried turn keeps
only the time of its last attempt, so the agent's time on a failed attempt counts as the
reviewer's.
