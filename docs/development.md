# Development

See the [README](../README.md#install) for building and installing the plugin.
Repository conventions and the feature workflow are in [AGENTS.md](../AGENTS.md).

## Checks

```sh
make check
```

In addition to the build dependencies, the checks use Herdr, Codex, Claude Code,
Python 3, `cargo-nextest`, `cccc`, and `jq`. Herdr integration tests run private
servers with isolated configuration, state and agent paths. The
[Explore page tests](#explore-page) also use Node and a headless Chromium, which
the Nix shell provides.

### The Herdr the tests run

The tests do not use the Herdr installed on your machine. The Nix shell fetches
a fixed Herdr release and points `TEST_HERDR_BIN_PATH` at it; the test servers
run that binary. `herdr` on the `PATH` stays the installed Herdr, which
`make install` uses. Outside the Nix shell, the tests fall back to `herdr` on
the `PATH`. Set `TEST_HERDR_BIN_PATH` inside the shell to try another binary:
`nix develop --command env TEST_HERDR_BIN_PATH=/path/to/herdr make check`.

Each test server also detects agents with the rules in
[`crates/review-test-support/agent-detection`](../crates/review-test-support/agent-detection),
copies of Herdr's published Codex and Claude rules. It installs them as local
overrides, which Herdr prefers to the rules built into its binary, so the fake
agents of the tests reach the same states whatever release runs them. The test
`agents_are_detected_with_the_rules_the_repository_keeps` fails when the pinned
release does not use these files, for example because a manifest requires a
newer rule engine than the release supports.

To change the pinned release:

1. In `flake.nix`, set `herdrVersion` and the `hash` of each `herdrAssets`
   entry. GitHub lists each asset's SHA-256:
   `gh release view v<version> --repo herdrdev/herdr --json assets --jq '.assets[] | "\(.name) \(.digest)"'`.
   Convert each one with `nix hash convert --hash-algo sha256 --to sri <hex>`.
2. If the release changes the agent detection, copy the current Codex and Claude
   manifests from `distribution/agent-detection` in the Herdr repository into
   `crates/review-test-support/agent-detection`.
3. Run `nix develop --command make check`.

The opt-in [`jev-evals` suite](jev-evals.md) compares Jev hunk-splitting strategies
against frozen line-level labels. Offline checks and paid live runs are separate;
neither runs during `make check`. The [history-based study](jev-history-study.md)
jointly compares prompts, metadata, exclusion rules and token windows on an
audited corpus from repository commits.

Optional real-language-server tests run in temporary projects:

```sh
cargo test -p review-lsp --test language_servers -- --ignored
cargo test -p review-ui real_rust_lsp -- --ignored --nocapture
```

Explore's domain, Git/jj comparison, durable inputs and isolated selected-agent MCP
tests use deterministic responses and call no model API. (The Explore page's e2e
tests in `make check` call a model only for a goal with no valid recording: see
[Agent steps and the model](#agent-steps-and-the-model).) Its optional UI
test uses real rust-analyzer to navigate working-copy sources and reject a
delayed result for another evidence window. Unchanged sources are resolved on demand without a repository catalog or capture. Explore assumes code stays unchanged during review;
it does not check freshness or suspend decisions after edits. The real-agent acceptance demo
is a separate manual check in a disposable repository and private Herdr server.
See the [slice 2 recovery report](explore-slice-2-recovery.md) for current acceptance evidence. The archived [adaptive demo](explore-adaptive-demo.md) predates durable resumption.

See [language server setup](language-servers.md) for the server commands, and
[mutation testing](llm-mutation-testing.md) for mutation-test guidance.

## Explore page

The Explore page is a browser page that shows an Explore round. Its routes,
templates and assets are in
[`crates/review-explore-page`](../crates/review-explore-page): axum serves HTML
rendered from minijinja templates, with no client framework. While the agent
works, and while a round starts, `assets/page.js` polls the page's
status and loads the page again once the round has changed. The reviewer's actions are form posts that need the
page's cookie: the page hands each one to the round's owner as a
`PageCommand`, waits for its reply, then redirects to the page (post, redirect,
get). A refusal travels to that next load in a short-lived cookie, which the
page shows once, worded by the template partial of its post
(`templates/notice-{post}.html`). Before it sends an answer, the page checks that the round
still asks the question it showed; before an Implement, that the conclusion
still offers it in place of the request the page showed, if any. The owner
checks again against its own round. The page also polls while an
implementation request is being sent. On a question whose Door is not two-way,
the page hides the recommendation until the reviewer's first pick
(`src/blind.rs`), unless the reviewer answered the question before and cancelled the
answer (`RoundStage::Question::answer_cancelled`): the pick is a form post that the
page keeps in a cookie, not a command, and the answer then carries it to the owner as
`AnswerInput::first_pick`. The comment typed with the pick stays in the page's memory
(`PickComments`), under an ID the cookie carries, until the answer is sent; an answer that is
not sent keeps its comment there. A conclusion with a quiz shows it first, one item at a time
(`templates/quiz.html`): the page grades a pick itself, saves it through the owner
(`PageCommand::Quiz`), then shows the item's answer at `/?answered=N` until the
reviewer moves on; once every item is answered or the reviewer skips the rest, the
conclusion shows with the results folded beside it.

The agent's Markdown (the design explanation, a question's Context, Door and
Blast radius, the conclusion) is rendered to HTML on the server by
[`crates/markdown-html`](../crates/markdown-html), which shows raw HTML as text
and keeps a fenced block's language as the class `language-<name>` of its
`<code>`. Callouts (`> [!TIP]`) and table-cell status marks (`[!good]`) are
defined once in [`crates/markdown-marks`](../crates/markdown-marks): the page,
the pane's renderer and the kickoff prompt all read them from there.

The page's layout by width is in `assets/layout.css`: one column on a phone, up to 84rem on a
wider window with text held to a readable measure, and from 70rem two columns for a question
and a conclusion, the reviewer's actions sticking beside the reading. It places the parts of a
question and a conclusion on a grid, so their templates keep one order, which a phone shows
as is.

A fenced `mermaid` block is a diagram, which `assets/diagrams.js` draws in the
browser with Mermaid, again with its dark theme when the page turns dark, at
the diagram's natural size in a frame that scrolls sideways. Mermaid is
vendored, pinned and gzipped at build time in
[`crates/mermaid-js`](../crates/mermaid-js) (its `vendor/README.md` says how to
move to another version); the page serves it itself, loads it only when it
shows a diagram, and lets the browser keep it, since its address names its
version. Mermaid writes inline styles into each diagram, so the page's content
security policy allows inline styles (`style-src 'self' 'unsafe-inline'`); it
still allows scripts only from the page itself. The tool cannot check a diagram
when the agent submits it: when Mermaid cannot parse one, the page shows its
source with Mermaid's message and posts the error to `/diagram-errors`, and the
Explore session saves it with the question (`Exploration::diagram_errors`).

In the reviewer, the Explore session publishes the stage of its round (no
round, a round starting or a failed start, the agent working, a question,
an interrupted turn, the conclusion)
after each input it handles, with what the agent's turn said back to the previous answer
(`TurnResponse`: its interpretations of the answer, with their recaps and follow-ups, and
its reply, the data the pane shows), and with the lines of the change that each citation of a
question names (`Comparison::tracked_cited_lines`, on the `cited_source` lookup of the pane's
evidence viewer, for files the change touches or the repository tracks only, since the page
may be open from the network), colored once per question on the session's thread by
[`crates/review-explore-citations`](../crates/review-explore-citations), and
[`crates/review-explore-page-host`](../crates/review-explore-page-host) serves
the page of that round on a free loopback port behind a new token. The worker names the
review the page belongs to after each snapshot (`ExploreSession::name_review`,
`RoundPublisher::name`), and the start screen shows it. The page's
commands join the session's inputs, in the same order as the pane's. For an
answer from the page, the session builds the turn from its latest saved round
as the pane would, so the saved answer and the prompt are the same, and it
refuses an answer to a question that has one already. A start from the page
replies at once, then captures the change and returns the kickoff to the
reviewer's worker (`ExploreSession::start_from_page`), which lets Jev mark
first as for a kickoff from the pane; the pane shows the round once the session
announces the saved kickoff. An Implement from the page builds the request as
the pane's does. Once the conclusion has a request that this process sends or
that the agent received, the session refuses another, from the page or from the
pane, so a repeated or stale Implement cannot start a second implementation; a
request an earlier process left paused or unknown stays the pane's to resolve.
The session's stage tells a request this process is sending from one an earlier
process left paused or unknown, and follows its delivery through the storage
watcher. The pane shows a request the page sent as its own. It records
the page's address, readable only by the user, under
`$HERDR_PLUGIN_STATE_DIR/explore-page/`, one record per Herdr workspace. The
Herdr action `explore-page` (`reviewer-control explore-page`) reads the record
of its workspace, checks that the page answers, and opens it with `$BROWSER`,
`xdg-open` or `open`.

The host also serves the page on a network interface, for a phone, on the same
thread and runtime: a second listener with its own host name and a new token for
each round (`PageHost::share`). While no round runs, the listener has a token for
the start screen, which the round started next keeps, so the page that started
it stays connected. It announces the page's address to the pane each time the
token or its round changes, and the pane draws it with its QR code
([`crates/ui-qr-code`](../crates/ui-qr-code)). When the listener cannot start, the pane
says why in place of the address (`ExplorePageNotShared`), with no toast. The settings
are in the [usage guide](usage.md#open-the-page-from-a-phone). Herdr test servers turn it
off (`HERDR_REVIEWER_EXPLORE_NETWORK=off`); a `make vision` session serves it on
the loopback interface, so the pane shows a QR code that only this machine can
open.

### Serve the page alone

```sh
nix develop --command make explore-page
```

This runs the standalone server
([`crates/review-explore-page-server`](../crates/review-explore-page-server)),
with no pane, no agent and no Herdr. It prints the address of a page that shows
a fixed question: `http://127.0.0.1:8790/?token=dev`. Open it in a browser.

The server reads the templates and assets from disk on every request, and an
open page loads itself again when one of them changes, so a markup or style edit
shows at once. A Rust edit needs a rebuild: stop the server and run the command
again, then reload the page. The token stays `dev`, so the page opens again
without a new address.

The server's own options: `--port N` (`0` picks a free port), `--token T`
(random when omitted), and `--dev DIR`, the page crate's directory to read the
files from. Without `--dev`, it serves the files built into the binary.

### e2e tests

The page is tested only with [e2e](https://github.com/tester-army/e2e), in
[`tests/explore-page`](../tests/explore-page). Write a test the way the e2e
documentation and its skill (`.agents/skills/e2e`) say. A test sets the round up
through the fixture below, then takes the steps the reviewer would take, each
followed by a check of its outcome:

- a step the reviewer would say in words is a goal for e2e's agent
  (`agent.act('pick "Discard the draft" as the answer to question 1')`), its
  actions recorded once with a model and then replayed;
- its outcome is checked by exact facts, each one locator and an assertion
  (`expect(screen.getByRole('radio', 'Discard the draft')).toBeChecked()`).

Check outcomes with locators, not with a judgement (`agent.assert`,
`agent.waitFor`, `agent.extract`), which calls a model on every run: every
outcome of this page so far is a text, a role, a state or a visible element,
Mermaid's drawn labels included. Should a locator ever be unable to check one,
keep a judgement, with the reason in a comment above it.

Check the facts that make the outcome, not each element of the page: listing
every choice, count and sentence with locators makes a test break on every
markup change. A matcher waits for the page, which follows the round a moment
after the fixture moves it. Locators address the page by roles and accessible
names, so the markup must name what a test looks for (a `<section>` labelled by
its heading, a `<fieldset>` with a `<legend>`, `role="status"`).

```sh
nix develop --command make e2e-explore
```

This target also runs during `make check`. It builds the standalone server and
runs every test at a desktop and at a phone size, each against its own server.
`E2E_ARGS` go to `e2e run`, with paths relative to `tests/explore-page`:
`make e2e-explore E2E_ARGS=tests/round.e2e.ts` runs one file, and
`E2E_ARGS=--no-cache` runs without the replay cache.
The first run in a checkout installs the npm packages from the committed
lockfile, which needs the network. The Nix shell provides Node and the headless
Chromium of nixpkgs (`E2E_CHROMIUM`), which the tests attach to instead of
Playwright's own download, and turns e2e's telemetry off
(`E2E_TELEMETRY_DISABLED`).

Each test gets its own round on the server, through the `explore` fixture of
`tests/explore-page/tests/session.ts`, and plays the Explore agent and the
reviewer's pane: the round starts with the agent working,
`explore.askQuestion()` posts the next question, `explore.answerInPane()`
answers it in the pane, `explore.cancelAnswerInPane()` cancels that answer (the question
then shows its recommendation at once),
`explore.failDelivery()` fails the prompt the session sends (the conclusion's
implementation request while it sends one, else the agent's next turn), and
`explore.interrupt()`, `explore.conclude()` and `explore.reset()` move the
round to the other stages. An answer sent from the page puts the agent to work,
and `explore.answers()` returns what the page sent; `explore.diagramErrors()`
returns the diagram errors the page reported, as the review tool saves them.
The design of the change carries a sequence diagram and a table that fit a desktop window
but not a phone's, and the design's diagram is the page's first (`Diagram 1`). The second
fixed question carries a diagram that draws and one that does not parse. From the second
question on, and with the conclusion, the agent's fixed response to the previous answer
shows above it. The session's review has a fixed name, which the start screen shows. A start sent from the page shows the round starting,
`explore.sendKickoff()` puts the agent to work on it (or stands for a round
started in the pane), `explore.failStart()` fails the start, and
`explore.starts()` returns the starts the page sent. An Implement from the page
shows the request as being sent until `explore.deliverImplementation()`;
`explore.implementInPane()` sends the conclusion's request from the pane, and
`explore.implementations()` returns the lists the page sent. `explore.concludeWithQuiz()`
concludes with a quiz of two items, and `explore.quiz()` returns what the reviewer
answered of it, as the review tool saves it. The server's control
routes are listed in
[`control.rs`](../crates/review-explore-page-server/src/control.rs); a
`question` step takes an optional JSON `Question` body for a question of the
test's own. The server's log of each
target is in
`tests/explore-page/.e2e/logs/`; a failed run prints its end. The run also
fails when the browser reports that the page broke its content security policy,
which allows scripts and styles only from the page itself, and inline styles
for Mermaid's diagrams. A failing test
leaves the accessibility tree of the page and a Playwright trace under
`tests/explore-page/.e2e/artifacts/`.

### Agent steps and the model

`make e2e-explore` uses e2e's replay cache the way
[its documentation](https://e2e.tester.army/docs/cache) describes. A goal with a
valid recording under `tests/explore-page/.e2e/cache` replays without a model
call. A new goal, or one whose replay no longer matches the page, goes to the
model, and the cache is updated once the check after the goal passes. The tests
make no judgement, which e2e never replays, so a `make check` whose goals all
replay calls no model, and passes on a machine with neither of the two routes
below.

The cache directory is committed, so a fresh checkout replays instead of paying
for the model again. Read changed entries like test data before committing
them: they hold the actions and the end state, never a prompt or a key. The run
summary gives the model calls and tokens (`AI ... tokens · N model calls`) and
what replayed (`Cache N replayed`). When a goal keeps going to the model on runs
with no change to the page, find out why instead of running again. To record
everything again, run `npx e2e cache clear` in `tests/explore-page` first.

`tests/explore-page/model.ts` is the only file that knows how the model that
acts out goals is reached. It has two routes, each with a fixed model, so
switching to a costlier model means editing that file. It sets no
[judge](https://e2e.tester.army/docs/models): a judgement would run on that
model. The first route available wins:

1. A ChatGPT subscription, through e2e's
   [subscription login](https://e2e.tester.army/docs/subscriptions).
   `gpt-6-luna` acts: it is the smallest model the subscription serves, a goal
   needs only tool calls and screenshots, and a small model uses the least of
   the plan's usage limits. Log in once, from `tests/explore-page` in the dev
   shell:

   ```sh
   nix develop --command npx e2e login openai
   ```

   Add `--device` on a machine without a browser. e2e keeps the login in
   `~/.config/e2e/oauth.json` (`$XDG_CONFIG_HOME/e2e/oauth.json` when that is
   set), readable by you only, and refreshes its tokens itself;
   `E2E_OAUTH_CREDENTIALS` holding the same JSON stands in for the file.
   `model.ts` only checks that an `openai` login is stored; a login file it
   cannot read or parse fails the run. `npx e2e models openai` lists the
   models the login serves. After `npx e2e logout openai`, goals use the
   second route.
2. Anthropic, with an API key: `claude-sonnet-5-5` acts. The key comes from
   `ANTHROPIC_API_KEY`, or else from the `ANTHROPIC_API_KEY` line of
   `~/.secrets` (`NAME=value` or `export NAME=value`):
   `tests/explore-page/run.sh` reads that line only, so the file's other
   secrets stay out of the tests' environment. If the key needs a workspace ID,
   set it in `ANTHROPIC_WORKSPACE_ID`.

With neither, a goal that needs the model fails at once and says so, and
replayed goals still pass. The run summary names the model, and the `AI` line
names the models that were called. The cache key does not hold the model, so a
recording made through one route replays under the other.

Run every `npx e2e` command inside the dev shell: outside it, e2e's telemetry is
on. `run.sh` refuses to run outside it.

### Write a test with an agent's help

The e2e skill, at `.agents/skills/e2e` (and `.claude/skills/e2e`), tells a
coding agent how to write and run e2e tests. It is what `npx e2e init` writes
for the pinned e2e version: when `package.json` moves to another e2e version,
run `npx e2e init --yes` in a scratch directory and copy its
`.agents/skills/e2e` over this one. Do not take the rest of what `init` writes:
its `.gitignore` lines would ignore the committed recordings, and its example
test fails in `make check`.

e2e's MCP server lets a coding agent open the page in a browser, act on it
(navigate, tap, type), read what it shows, and try a locator with `locate`
before writing it into a test. Its sessions record nothing: `make check` gains
no recording from them. Register it once for Claude Code, from the repository
root:

```sh
claude mcp add e2e -- "$PWD/tests/explore-page/mcp.sh"
```

`tests/explore-page/mcp.sh` enters the Nix dev shell when it is not already in
it, then starts the server with this project's e2e and
`tests/explore-page/e2e.config.ts`, so it works from any directory. Install e2e
once first with `nix develop --command make e2e-explore-deps`.

An agent whose session has no server registered can drive the same server from
the shell: `tests/explore-page/mcp-cli.mjs` starts `mcp.sh`, runs the tool calls
given as arguments in one session, prints each result, then closes the session:

```sh
nix develop --command node tests/explore-page/mcp-cli.mjs \
  '{"tool":"open_session","args":{"target":"desktop"}}' \
  '{"tool":"call","args":{"tool":"navigate","args":{"url":"/?token=e2e"}}}' \
  '{"tool":"call","args":{"tool":"locate","args":{"role":"radio","name":"Keep the draft"}}}'
```

`locate` answers with the locator a test would use, such as
`screen.getByRole("radio", "Keep the draft")`. `/?token=e2e` opens the server's
own round, which shows the fixed question. Each run of `mcp-cli.mjs` starts a
new page server, and nothing it does is recorded.

## Terminal UI exploration

The Rust harness in [`tests/tui`](../tests/tui) runs the real reviewer in a
`tui-test` PTY. It reuses the Git/jj repository and Herdr fixtures from
`review-test-support`, with private sockets, config, state, workspaces, and MCP
ports. Use LLM explorations to discover cases for the regression suite.

The pinned `tui-test-rs` beta requires Rust 1.90, provided by the Nix shell.
The harness has its own Cargo workspace and lockfile because `tui-test` requires
`unicode-width` 0.2.2 while the existing Ratatui 0.29 adapter pins 0.2.0.
`make vision` builds the reviewer first and supplies `REVIEWER_BIN_PATH` to the
driver. Its private Herdr server runs the [pinned release](#the-herdr-the-tests-run).

### A second checkout

`/tmp` may be a RAM disk, and a full build needs about 16 GB. Put a second jj
workspace or checkout on disk, or point `CARGO_TARGET_DIR` at the main
checkout's `target/`, before running `make check` in it.

### LLM-directed exploration

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
`claude`, which Herdr detects as an agent, swallows every prompt the reviewer
sends. The reviewer itself records each Explore prompt, once its delivery
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

`explore_page` runs the Herdr action that opens the [Explore page](#explore-page)
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

### Regression tests from explorations

Populate [`tests/tui/tests`](../tests/tui/tests/README.md) with deterministic E2E
cases derived from recorded LLM explorations. Keep each case focused on a
reproduced finding, with the expected behavior and relevant interaction sequence.
Preserve the evidence needed to understand the case alongside the test; session
artifacts under `target/` are ignored by version control.

Run the harness checks and any added regression cases with:

```sh
nix develop --command make e2e-tui
```

This target also runs during `make check`. It builds the reviewer, checks the
Rust harness, and runs its tests. The current tests cover frame-capture plumbing;
UI scenarios will be added from explorations.

## Diagnose UI stalls

Set `HERDR_REVIEWER_TIMINGS` to a JSONL file path when starting the reviewer.
It records event queue delays, handler times and frame render times.

### Adaptive Explore protocol

The authoritative agent contract is `crates/review-explore-runner/src/interview.md`.
One `InterviewUpdate` binds a direct reply, optional decision interpretation,
agenda operations and one next question to the exact
outstanding request. `Exploration` validates all citations and the
agenda before applying anything. `submit_question` carries these turns;
`submit_conclusion` carries a separate `ConclusionSubmission` with summary, editable
implementation tasks, future work, the quiz and final-answer interpretation. Both tools route through the
existing reviewer server to a locked store transaction and then the UI; success acknowledges
validation, durable commit and application. Invalid updates remain pending and return their error
to the agent for repair. Cancelled/obsolete requests cannot apply, and an exact
duplicate is acknowledged without replaying it. Replacing an accepted payload is
rejected. Thread comments, Explore turns and guide requests submit directly through Herdr's
`agent.prompt`. Delivery does not parse terminal output or wait for an idle agent,
unfocused pane, or empty composer. Review access binds to the native session when
available, or the foreground process group otherwise. Cancellation removes unsent
requests and a replaced conversation rejects them.
Delivery failures retain the literal input for Retry. The kickoff prompt carries the scope, the change description (jj only; Git has none) and the first turn identity; the agent submits
its first question directly, after the design explanation in `design`, which only the first turn
may carry and its question must carry. The pane shows it before the first question, and the page
shows it open above the first question and folded in every later stage of the round. The tools advertise complete schemas derived from the
shared submission types; the kickoff carries behavior instructions without schema
examples. The runner formats later wakeups as the later-turn rules (`wakeup.md`: interpretation,
review marks, agenda changes, concluding) followed by labeled plain text: turn identity,
one checkpoint, answer ID, question ID/version, the
selected option's full text/ID/outcome and any exact comment, after the Unreviewed
diffs line. Conclusion context and previous errors add details only when relevant. Full questions
and answer records stay internal; preparation does not mutate that history. No input-fetch
tool or separately retained runner input is needed. The reviewer retains the immutable history while
the agent keeps its context in the existing conversation. A contribution to a questionless stopping
point carries Reply to conclusion identifying its closing turn;
that context cannot be interpreted as a policy decision. Session discovery/replacement handling
remains independent of repository freshness.

Agenda lifecycle (retire/supersede/reconsider) is separate from decision status.
Topics hold pending wording, order and prerequisite topic IDs; posted versions and
agent operations are retained in conversation history. Reconsideration names the
original decision and a subsequent attributed decision resolves the flag.
Interpretations are optional for informational contributions. The protocol cannot
mechanically establish whether free text is agreement or whether an assessment is
true: visible replies/recaps, evidence, follow-up answers and human inspection remain
necessary. It validates reference integrity, not the agent's semantic reasoning.

`CodeLocation` carries a repository-relative path, old/new side and optional inclusive
line range. Paths use UTF-8 strings or byte arrays for non-UTF-8 names. There are no
source IDs or source-registration calls. Topic associations use the same locations.
The internal comparison retains changed-file context for native viewers without
listing all repository files. Unchanged sources are resolved on demand; historical
files use the original base and new-side files are read directly. Agents inspect
files on disk and use Git/jj for diffs and historical versions. In Git, the review
unit identifies the base tree; in jj, the checkpoint identifies the reviewed commit.
Submissions require the exact request ID, access value and pinned conversation.
Answers arrive through the shared prompt delivery; no handoff files are created.
Question evidence is one ordered list of citations, most decisive first; citations
from replies, agenda reasons and consequence lenses appear after it. Every citation
requires `notes` that explain its relevance. Citations never mark lines reviewed. Citations
are deduplicated by path, side and range, retaining stable viewer
identities. Every next question requires two to five distinct alternatives.
`Question::choices` adds the built-in None of the above choice, with a stable ID
and open outcome, without rewriting the posted question. Its ID and label are
reserved so agent alternatives cannot duplicate it. Choice selection and text
editing share one answer: Send records both. The question is rendered directly above this form.
Additional LSP destinations are also read inside the root. Historical evidence
outside diff hunks uses a native full base-text view and cannot send old coordinates
to the live language server. Evidence sizing and initial positioning use the same
wrapped range, including yellow borders; fitting again recenters the range rather
than its first line.


Explore renders only the selected question, with a pinned history navigation bar.
New accepted questions select the newest page; ordinary input no longer suppresses
advancement. Applying a duplicate does not navigate. Files and Threads retain their
active pane while Explore prepares its next page. Question identity also selects
the composer draft and evidence window; history navigation saves the full editor
state before restoring the destination draft.

Conclusion pages are retained by request identity, in posting order alongside
questions. Each keeps its task editor, reply draft and delivery state. Only the
current conclusion can start implementation. The explicit Implement action sends
only the human-edited task list through the shared prompt queue; the result event
confirms delivery, not completed implementation.


### Durable Explore recovery

`review-explore::ExploreRound` stores the existing domain types and delivery records;
`ExploreViewState` stores portable editor/reading state. `Comparison` omits live
source buffers and diff caches from serialization. Restored evidence lazily reopens
only the selected file through working-copy/history readers, including native diff
rendering. Missing paths, ranges or base revisions are local evidence limitations.

`review-store` uses `explore-v1/<logical-review-hash>/` inside its canonical-checkout
namespace. Each round and its deduplication state share an atomic JSON record. A
per-review lock protects mutations against the latest revision; archived rounds
accept only completion of already recorded dispatch attempts. `index.json` retains
round order. One `<round-id>.view.json` record stores the reviewer's editors and reading
position without rewriting domain history. Its save sequence continues across
reopening. Explore assumes one agent and one reviewer per repository, with no window
identities, alternate draft sets, or legacy-format migration. Record bounds are
256 MiB for a round and 16 MiB for an editor/index; readers reject invalid versions,
structure and oversized data without
replacing it. Writes sync files and parent directories. The existing repository
watcher's lifecycle also observes domain-state filesystem events; no polling or
per-keystroke history serialization was added.

The application queues coalesced editor saves on the worker before dependent
actions. Normal shutdown drains the worker queue. Unflushed keystrokes can be lost
on abrupt death; acknowledged domain changes and authorizations cannot depend on
that queue flush. Posted answers retain their exact original option and comment.
Explicit Retry reuses the logical request ID with a fresh dispatch-attempt ID, so
a late cancellation cannot fail a newer attempt.

`DispatchObserver` records durable outcomes around the shared `agent.prompt`
submission. Before external delivery, it commits `Attempting`; a lost result
recovers as `Unknown`. A confirmed result wins cancellation races and can be saved
to an archived round. Queued work stays paused on restore, and retries cannot alter
its authorized scope. Runtime access, connections and caches are never persisted.
Native identity compares agent/kind/value, allowing a resumed pane; unresolved
original bindings and actual replacement conversations fail closed for continuation.
