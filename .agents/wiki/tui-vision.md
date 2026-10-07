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
`HERDR_SOCKET_PATH`.

The server lives in [`tests/tui`](../../tests/tui), its own Cargo workspace because
`tui-test`, which draws the screenshots, requires `unicode-width` 0.2.2 while Ratatui 0.29
pins 0.2.0. `start` builds the reviewer of the checkout in the dev shell, so each session
runs the current code.

## The session's workspace

`start` runs the reviewer in a new "reviewer vision" workspace of the user's live Herdr,
beside a stand-in agent, and the user switches to it to watch. The session has a workspace
of its own because the reviewer prompts the last focused agent of its workspace: in the
user's workspace, that could be the user's real agent. The reviewer takes the tab's size,
which follows the user's terminal: Herdr sets split ratios, not a pane's size in cells, so
the size can differ between runs (199, then 200 columns). Click by text, or read `size`
first.

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
- The stand-in agent reports its state to Herdr itself, as Claude's hooks do: the user's
  Herdr may not read its working state from its titles.
- The stand-in agent reports no session and runs no hook of the reviewer's Claude Code plugin,
  so run-ahead takes no fork of it. Reporting a session for its pane once it took the kickoff,
  with `herdr pane report-agent-session <pane> --source herdr:claude --agent claude --seq
  <nanoseconds> --agent-session-id <id> --session-start-source startup`, shows run-ahead's
  reason for an agent without the plugin; Herdr then ignores the stand-in's own state reports,
  and a report before the kickoff keeps it from starting on it.

A whole Explore round in the pane, with the arguments of
[`tests/tui/examples`](../../tests/tui/examples) (the reviewer's tools reject unknown and
missing fields, so start from these files): `click` "Explore", then "Start in the pane";
`turn` returns the kickoff; `reply` with `question.json` shows "Question 1"; `key` `1`, then
`ctrl+enter`, sends the answer; `turn` returns the wakeup; `reply` with `conclusion.json`
shows "To be implemented".

## Regression tests from explorations

Populate [`tests/tui/tests`](../../tests/tui/tests/README.md) with deterministic E2E
cases derived from recorded LLM explorations. Keep each case focused on a reproduced
finding, with the expected behavior and relevant interaction sequence. Preserve the
evidence needed to understand the case alongside the test; session artifacts under
`target/` are ignored by version control. A test drives the reviewer in a private Herdr
(`HerdrTestServer`), never the user's.

`make e2e-tui` runs them with the server's own tests: its logic, and sessions in a private
Herdr (`src/vision/session.tests.rs`) that wait on Herdr's events.
