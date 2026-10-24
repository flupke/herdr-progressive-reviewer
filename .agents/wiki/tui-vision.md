# Terminal UI exploration (make vision)

How an agent drives the real reviewer pane through the vision MCP server, in the user's Herdr, how its tools wait for the reviewer, its gotchas, and turning findings into regression tests.

The vision MCP server is an agent's eyes and hands on the review pane. Its tools and their
schemas say what each one takes and returns; this page holds what they cannot say.

## Setup

Run `make vision` first: it builds the server and the reviewer, and prints how to use them.
`.mcp.json` registers the server as `reviewer-vision` through `scripts/vision-mcp`, which
runs the built server, or builds it first when it is missing or older than the sources
(slower, and it says so on standard error). Claude Code asks once to approve a project's
server. The agent must run inside Herdr: the server reaches the user's Herdr through
`HERDR_SOCKET_PATH`. A session whose server was missing or stale when Claude Code connected may
get its instructions but no tools: run `make vision`, then
drive the server over stdio with a small MCP client (`initialize`, then `tools/call`), from one
process, since a client that leaves ends the session.

The server lives in [`tests/tui`](../../tests/tui), its own Cargo workspace because
`tui-test`, which draws the screenshots, requires `unicode-width` 0.2.2 while Ratatui 0.29
pins 0.2.0. `start` builds the reviewer and the stand-in agent of the checkout in the dev shell,
so each session runs the current code. The server itself does not rebuild: an MCP client keeps
the process it started, so after a change under `tests/tui` the client must start the server
again to get it.

## The session's workspace

`start` runs the reviewer in a new "reviewer vision" workspace of the user's live Herdr,
beside a stand-in agent, and the user switches to it to watch. The session has a workspace
of its own because the reviewer prompts the last focused agent of its workspace: in the
user's workspace, that could be the user's real agent. The reviewer takes the tab's size,
which follows the user's terminal: Herdr sets split ratios, not a pane's size in cells, so
the size can differ between runs (199, then 200 columns). Click by text, or read `size`
first.

The stand-in agent is the tests' ([test stand-ins](test-stand-ins.md)), named `claude`: it
reports its session to Herdr as Herdr's own Claude Code hook does, runs the hooks of the
reviewer's Claude Code plugin through the checkout's `reviewer-control`, and plays a fork when
run-ahead starts it as one. The reviewer and the agent run with a runtime directory of their own,
where the reviewer hears the hooks (`runtime` in `start`'s result, a temporary directory since a
socket's path must be short), and the agent and its forks keep their transcripts in
`claude-config` in the session directory: a session touches neither the user's reviewers nor
their Claude Code configuration.

`stop`, or a client that leaves or signals the server, closes the workspace only while it
is still the session's: its label, and no pane the session did not open. A workspace the
user renamed, or moved a pane into, stays open, and `stop` says why. `reopen` stops the
reviewer and waits for Herdr to report its process ended before it starts the new one, which
binds the same MCP port.

## How the tools wait

In a vision session the reviewer paints each frame as one synchronized update and ends it
with a terminal title, its frame marker (`crates/vision-signal`): `reviewer vision frame=N
ack=K size=CxR`. Herdr reads the title after the frame in the pane's byte stream, and
reports it as a pane update event. After its input, an action pastes an acknowledgement
request that the reviewer never hands to the application; the reviewer names it in the next
frame's marker. So an action returns the rows that changed on the screen of the frame that
followed its input, with no delay of its own. Asynchronous work (a refresh, a diff loading,
Jev, an agent's turn) shows later: `wait_for` it, which checks the screen at once, then after
each frame.

## Gotchas

- The first screen `start` returns can come before the repository has loaded: `wait_for`
  a file name first.
- `click` with `text` clicks the first place the screen shows it, in row order: `notes.md`
  in the diff's title comes before the file list. Pick text that shows once, or a cell.
- While the reviewer has handed the terminal to `$EDITOR`, the editor reads the input and
  the acknowledgement request, and no frame names the request: an action then fails after
  its guard. Quit the editor through its own keys before acting on the reviewer again.
- On the Explore tab, `Start` runs the round on the Explore page and `Start in the pane`
  in the pane.
- A `drag` copies a text selection (over code in the diff or an Explore evidence viewer, only
  with `alt`), and Herdr hands the copy to the clipboard of the user's own terminal: save it
  first and put it back. The clipboard may hold no text at all (a browser's `text/html`), so
  save it by type (`wl-paste --list-types`, `wl-paste -t <type>`) and restore it with
  `wl-copy -t <type>`. Each client of the server ends its session when it leaves, so drive a
  whole check, and read the clipboard between steps, from one client.
- Herdr shows a run of blank cells that ends a row in the style of the cell before it once
  that cell is redrawn, though the reviewer never writes them: a text selection that ends
  before trailing spaces looks highlighted to the end of the line's padding. The selected
  cells are right; ratatui sends no erase sequence (inferred: Herdr's grid).
- Herdr reads the stand-in agent's state from its titles, Claude Code's (`✳ Ready`, `⠋
  Working`), and the agent reports none itself: once a session is reported under
  `herdr:claude`, Herdr 0.9.5 ignores the state an agent reports under a source of its own, but
  still reads its titles when nothing else reports its state (probed in the user's Herdr).
  A turn of the agent ends once Herdr saw it working; the status then reads `done` until the
  pane is seen, which the reviewer takes as idle.

A whole Explore round in the pane, with the arguments of
[`tests/tui/examples`](../../tests/tui/examples) (the reviewer's tools reject unknown and
missing fields, so start from these files): `click` "Explore", then "Start in the pane";
`turn` returns the kickoff; `reply` with `question.json` shows "Question 1"; `key` `1`, then
`ctrl+enter`, sends the answer; `turn` returns the wakeup; `reply` with `conclusion.json`
shows "To be implemented".

A review comment answered by the agent: `key` `tab`, then `j` to a changed line, then `a`;
`type` the comment, then `ctrl+enter` posts it; `turn` returns its `comments` wakeup;
`reply` with `get_new_messages` and `{}` returns the threads, then `reply` with `reply` and
the fetched `thread_id` and `in_reply_to`, a new `message_id` and the `text`; `key` `t`
shows the answer in the Threads tab. The thread tools take only the access value, which
`reply` fills in from the latest turn: when that is an Explore turn, pass the `comments`
turn's `access` as `review`.

## Run-ahead in a session

In the Explore tab, `key` `z` twice before the round starts: the pane shows `Run ahead
(experimental): every choice` (once, `the recommended choice`). After a question, `forks` with
`count` waits until that many forks started, and lists each fork's session, choice, prompt (its
access and Answer ID among the turn's files) and how far it got, the agent's session, and
run-ahead's log (`run_ahead_log` in `start`'s result, `state/run-ahead.log` of the session
directory). `fork_submit` plays a fork: it submits with the fork's own access, then the fork's
output says its submit has its answer, as Claude Code's would. Then `key` `1`, then
`ctrl+enter`, answers bare: the pane and the Explore page show `Prepared while you were
thinking`, and `forks` shows the agent on the fork's session, which the next question's forks
copy and the next answer reaches. An answer with a comment runs the plain chain and shows `Not
prepared: you added a comment`. A bare answer before `fork_submit` waits for the fork: the pane
shows `The agent is finishing a turn it started while you were thinking` with a running time,
and `fork_submit` then shows the fork's turn; `Stop waiting` gives the answer to the agent, whose
next `turn` returns it.

- The arguments of [`conclusion.json`](../../tests/tui/examples/conclusion.json) make a fork's
  turn a conclusion. A fork's question must ask about a topic not decided yet: the reviewer
  refuses one on the topic just answered (`Do not ask again on a topic just decided`), and a
  topic's `status` is `open`, `accepted` or `needs_follow_up`.
- `forks` lists every event the stand-ins reported by the time it returns: read the agent's
  sessions there after the action you wait for, since a Herdr report comes before the event.
- The Explore page is a client that draws itself from a socket: `curl` shows no round. Open its
  address (`explore_page`) in the dev shell's headless Chromium (`E2E_CHROMIUM`), with Playwright
  from `tests/explore-page` once `make e2e-explore-deps` installed it, or read its text with
  `"$E2E_CHROMIUM" --headless --virtual-time-budget=8000 --dump-dom <address>`.

### Scripting the forks and the switch

Per question, the driver chooses what each fork does, once `forks` listed it:

- submits its turn: `fork_submit`;
- is still working at the answer: nothing; the bare answer waits for it;
- ends without a turn: `fork_end`; crashes: `fork_end` with `crash` (exit status 1, no result).
  Both count a failure of run-ahead, as a switch that fails does: the third in a row stops
  run-ahead for the round, with a toast, and the round's later answers say `nothing was
  prepared`. Only a prepared turn used starts the count again;

and before the answer, what the switch to a fork does: `switch_unheard` makes the agent's next
`/resume` unheard, so the switch fails after its 20 s, the turn waits for Retry (`r`) and the agent
resumes its own session; with `heard_after_ms` it is heard that late and succeeds. Stop waiting
and Cancel answer play while it waits.

How to reach each `Not prepared` line, the same in the pane and on the Explore page:

| Line ends with | How |
| --- | --- |
| restart the agent to load … plugin | `start` or `reopen` with `agent_hooks: false` |
| you added a comment / you answered None of the above | the answer itself |
| you sent the agent back to the drawing board / you wrote in the chat | sent from the Explore page, which alone has both; the pane shows the line too |
| the agent was working | `agent_working` true before the answer, false after it |
| the agent's conversation moved on | `agent_prompt` after the forks started |
| the lines left to review changed | a new changed line in the scratch repository |
| nothing was prepared for this question | run-ahead stopped for the round, or the agent working when the question came |
| only the recommended choice is prepared | setting `the recommended choice`, another choice answered |
| the turn for this choice did not finish | `fork_end`, before the answer or while it waits |
| you stopped waiting for it | Stop waiting while the answer waits for its fork, or during the switch after it |
| the agent could not switch to the prepared turn | `switch_unheard`, then Retry |
| you stopped waiting while it was being used | `switch_unheard` with `heard_after_ms`, Stop waiting during that switch, then Retry |

Two lines have no way there: `the round changed after it was prepared`, since every input of a
turn but the unreviewed lines is fixed when the round starts, and `the turn for this choice was not
ready yet`, which only an earlier version recorded.

## Regression tests from explorations

Populate [`tests/tui/tests`](../../tests/tui/tests/README.md) with deterministic E2E
cases derived from recorded LLM explorations. Keep each case focused on a reproduced
finding, with the expected behavior and relevant interaction sequence. Preserve the
evidence needed to understand the case alongside the test; session artifacts under
`target/` are ignored by version control. A test drives the reviewer in a private Herdr
(`HerdrTestServer`), never the user's.

`make e2e-tui` runs them with the server's own tests: its logic, and sessions in a private
Herdr (`src/vision/session.tests.rs`) that wait on Herdr's events.
