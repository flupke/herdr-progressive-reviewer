# Terminal UI exploration (make vision)

How to drive the real reviewer pane in a private session: the `reviewer-vision` driver, its commands, what a session records, and turning findings into regression tests.

The Rust harness in [`tests/tui`](../../tests/tui) runs the real reviewer in a
`tui-test` PTY. It reuses the Git/jj repository and Herdr fixtures from
`review-test-support`, with private sockets, config, state, workspaces, and MCP
ports. Use LLM explorations to discover cases for the regression suite.

The pinned `tui-test-rs` beta requires Rust 1.90, provided by the Nix shell.
The harness has its own Cargo workspace and lockfile because `tui-test` requires
`unicode-width` 0.2.2 while the existing Ratatui 0.29 adapter pins 0.2.0.
`make vision` builds the reviewer first and supplies `REVIEWER_BIN_PATH` to the
driver. Its private Herdr server runs the [pinned release](checks.md#the-herdr-the-tests-run).

## LLM-directed exploration

Start a persistent reviewer session for an agent to observe and operate:

```sh
nix develop --command make vision
# Select Git instead of the default jj fixture:
nix develop --command make vision VISION_ARGS='--repo git'
```

The Rust `reviewer-vision` driver starts a private Herdr server and a temporary
repository containing several changes, including Unicode and a long line. It
prints the actual terminal text and stays alive, accepting one JSON command per
stdin line. Each interaction returns the latest completed screen and frame ID.

An agent whose shell has no persistent stdin runs the driver in the background
with `--commands PIPE`: the driver creates that named pipe, reads commands from
it, and reopens it after each writer, so every command is one `echo`. Responses
go to stdout, so redirect it to a file and read its last line. Redirect
the log with `>|`: under zsh's `noclobber`, `>` refuses an existing log, so the
driver never starts.

```sh
nix develop --command make vision \
  VISION_ARGS="--json --output $DIR/session --commands $DIR/cmd" >| $DIR/out.log &
tests/tui/vision-send $DIR '{"action":"click","text":"notes.md"}'
```

`tests/tui/vision-send` writes one command to the pipe, waits for the driver's
answer and prints its status, any error, `reply` or turn, and the screen text;
`VISION_RAW=1` prints the answer's JSON. Use it instead of writing to the pipe
and reading the log by hand. A command starting with `@` is read from that
file, which may span several lines: `vision-send $DIR @tests/tui/examples/question.json`.

Send `stop` when the exploration is done: it closes the live viewer and frees
the private server. A driver left behind stops by itself after 30 minutes
without a command; `--stop-after-idle MINUTES` changes that, and `0` waits
forever. An idle stop answers with `"reason": "idle"`.

```json
{"action":"observe"}
{"action":"press","key":"?"}
{"action":"press","key":"Escape"}
{"action":"type","text":"A comment with café and 日本語"}
{"action":"click","x":5,"y":3}
{"action":"click","text":"notes.md"}
{"action":"resize","cols":70,"rows":20}
{"action":"observe","after":12,"timeout_ms":3000}
{"action":"wait","text":"Jev: marked","timeout_ms":5000}
{"action":"cells","x":0,"y":0,"width":10,"height":1}
{"action":"screenshot"}
{"action":"note","kind":"checked","text":"Help closes with Escape and restores the diff"}
{"action":"jev","path":"src/math.rs","lines":[2]}
{"action":"turn","timeout_ms":15000}
{"action":"reply","turn":1,"tool":"submit_question","arguments":{"update":{...}}}
{"action":"explore_page"}
{"action":"reopen"}
{"action":"stop"}
```

Coordinates are zero-based terminal cells. `click` with `text` clicks the first
place the screen shows it, so no column needs counting across wide characters. `type` sends bracketed paste, including
newlines, through the application's editor. `observe` can wait up to 30 seconds
for a frame newer than `after`; `unchanged` explicitly reports a wait without a
new frame. An interaction waits up to one second and allows 100 ms for a changed
screen to settle. A changed frame is evidence to inspect, not proof that the
requested action has finished. For asynchronous work, `wait` for the text the
finished screen shows: it returns as soon as the screen shows it, with status
`shown`, or after `timeout_ms` (default 5000, at most 30000) with status
`timeout`. It checks the current screen first, so wait for text the screen
before the command did not show. Waiting on text instead of a fixed delay keeps
a fast, scripted exploration in step with the UI.
`cells` reports the captured cells' styles when focus or selection is conveyed by
color. `screenshot` saves a PNG of the screen, rendered with a bundled JetBrains
Mono, as `screenshots/frame-N.png` and returns its path: use it to judge how the
UI looks (spacing, alignment, theme), and the text frames for everything else. `jev` stands in for the paid Jev classifier, which vision sessions never
reach: it classifies as insignificant every diff hunk of `path` that adds one of
the one-based `lines` (current numbering) or removes one (base numbering), then
presses `rf` and returns the screen with its "Jev: marked" result, or after ten
seconds. Nearby edits share one diff hunk, as they do for Jev. The script it
writes is kept as `jev-script.json` in the session directory. Write that file
yourself before starting an Explore round to have Jev mark at the round's start.

The reviewer's `BROWSER` is a stand-in too, so Start never opens the browser of
your desktop: `stand-in-browser` in the session directory appends each address it
opens to `opened-pages` there, and fails with exit status 3 while a file
`browser-fails` exists there, to show what the pane says when the browser cannot
open the page.

The workspace's first pane is a stand-in implementation agent: a script named
`claude`, which Herdr detects as an agent, reads every prompt the reviewer
sends and shows Herdr a working title for two seconds, so the prompt counts as
started. Create the
file that `session.json` names as `agent_swallows_prompts` to make it read each
prompt without starting on it, as an agent that drops a paste does; remove the file
to make it start again. The reviewer itself records each Explore prompt, once its delivery
finished, as a numbered turn (`turns/turn-000001.json` in the session
directory; `HERDR_REVIEWER_VISION_TURNS` names it). This is test tooling: the
reviewer records turns only in a vision session (`HERDR_REVIEWER_VISION`), as
the files hold access values, and a normal reviewer ignores the variable. `turn` waits for the next
turn and returns it: its number, `kind` (`kickoff`, `wakeup` or `implement`),
whether it was `delivered`, the access value and identity a reply needs, the
reviewer's `answer`, the cancelled answers, the `unreviewed` line naming the diffs directory, and the full
prompt `text`. Each call returns the turn after the last one it returned;
`after` asks from another number. `reply` answers a turn by calling an MCP tool
on the reviewer's real endpoint as that agent: it fills in the turn's access and
the `instance`, `request` and `checkpoint` (inside `update` for
`submit_question`) unless `arguments` sets them, gives an `interpretation`
without an `answer` the answer the turn brought, and returns the tool result as
`reply` beside the screen. It refuses a `turn` that is not the latest, so a
script cannot answer a prompt the reviewer has replaced. `arguments` that bring
their own `review` value are sent as they are, to check how the reviewer itself
rejects a late or misplaced turn.

A whole Explore round, from the examples in `tests/tui/examples`. The tools
reject unknown and missing fields, so start from these files:

```sh
send() { tests/tui/vision-send "$DIR" "$@"; }
send '{"action":"click","text":"Explore"}'
send '{"action":"press","key":"s"}'                # Start
send '{"action":"turn","timeout_ms":15000}'        # the kickoff, turn 1
send @tests/tui/examples/question.json             # the agent asks
send '{"action":"wait","text":"Question 1"}'
send '{"action":"press","key":"1"}'                # choose the first option
send '{"action":"press","key":"ctrl+enter"}'       # send the answer
send '{"action":"turn","timeout_ms":15000}'        # the wakeup, turn 2
send @tests/tui/examples/conclusion.json           # the agent concludes
send '{"action":"wait","text":"To be implemented"}'
```

Keys are named as Herdr names them (`ctrl+enter`, `PageDown`, `Escape`, `Tab`);
a name Herdr does not know is typed as text. A button is clicked by its label
when no other text on the screen contains it, by coordinates otherwise.

`explore_page` runs the Herdr action that opens the [Explore page](explore-page.md)
(`reviewer-control explore-page`) as Herdr runs it in the session's workspace,
with a `BROWSER` that records the address instead of opening a browser, and
returns that address, with its token, as `page`. Open it with curl
(`curl -sL -c jar -b jar "$PAGE"`) or in a browser on the same machine.

`reopen` starts
the reviewer again in the same private workspace and state, at its initial
100×30 size. `stop`, SIGINT, SIGHUP, and SIGTERM clean up the reviewer, private
server, and temporary repository; so does EOF on stdin when the driver reads
commands from it.

When the driver runs inside Herdr, it splits its own pane and runs a live viewer
there: every frame it captures is painted in the viewer the moment it is
published, so a person can watch the agent explore. The viewer splits the
driver's pane the way the reviewer splits the pane it opens from, across the
side that looks longer (beside a pane at least two and a half times as wide as
it is tall, below any other), and takes the share that shows the whole 100×30
session, at most three quarters of the pane. `--viewer right|down|none` picks
the direction or turns the viewer off, and `--viewer-ratio` the viewer's share
of the split. The viewer shows the session at its own size: a narrower pane cuts
rows at its right edge, and a shorter one paints the rows that do not fit over
its last row. Keys typed in the viewer do not reach the session; `q` closes the
viewer and its pane and leaves the session running. The pane closes when the
driver exits, however it exits: the viewer closes its own pane once the stream
ends, and the system ends the stream of a killed driver too. Outside Herdr,
watch the same stream from any terminal with `reviewer-vision --view SOCKET`,
using the `stream` socket that `session.json` names.

Every session gets a new directory under `tests/tui/target/vision/`, printed with
the observations. `--output NEW_DIRECTORY` selects another location and `--json`
returns JSON lines for automated clients. The directory retains:

- `latest.txt`: an atomically replaced text screen with its frame ID, dimensions,
  and cursor metadata, refreshed even while the agent is thinking.
- `frames/`: the most recent 64 distinct text screens. Style and cursor changes
  count as distinct frames even when the characters stay the same.
- `actions.jsonl`: commands, observations, errors, and agent notes. Note kinds are
  `checked`, `finding`, and `untested`; findings should include expected behavior,
  observed behavior, and reproduction steps.
- `recording-*/`: `tui-test`'s textual asciinema recordings, retained across reopen.
- `screenshots/`: the PNGs `screenshot` saved, named after their frame.
- `session.json`: the private repository path, the live `stream` socket and
  session details. An agent can
  edit files in that repository to exercise filesystem-driven updates.

The driver sets `HERDR_REVIEWER_VISION=1` on its child. In that mode the reviewer
marks completed paints with terminal synchronized-update sequences. A filesystem
watcher follows `tui-test`'s recording and replays it through the library's terminal
emulator, publishing text only at those boundaries. This preserves complete frames
even when one paint spans several PTY reads, or several paints share one read.
Unchanged paints are deduplicated. Capture uses filesystem events rather than
periodic screenshot polling. The private child clears `NO_COLOR` so style
inspection includes the UI's normal selection and focus colors.

Give the agent a feature contract and an exploration objective. Let it choose
actions from what it sees, record checks and findings with `note`, and finish with
the untested areas.

## Regression tests from explorations

Populate [`tests/tui/tests`](../../tests/tui/tests/README.md) with deterministic E2E
cases derived from recorded LLM explorations. Keep each case focused on a
reproduced finding, with the expected behavior and relevant interaction sequence.
Preserve the evidence needed to understand the case alongside the test; session
artifacts under `target/` are ignored by version control.

Run the harness checks and any added regression cases with:

```sh
nix develop --command make e2e-tui
```

This target also runs during `make check`. It builds the reviewer, checks the
Rust harness, and runs its tests. Its tests cover the frame-capture plumbing and the
live viewer; UI scenarios come from explorations.
