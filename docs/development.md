# Development

See the [README](../README.md#install) for building and installing the plugin.
Repository conventions and the feature workflow are in [AGENTS.md](../AGENTS.md).

## Checks

The checks come in levels, cheapest first. Run the ones that fit the change
([AGENTS.md](../AGENTS.md) says when):

| Level | Command | What it runs |
|---|---|---|
| 1 | `make lint` | clippy on every target, and the complexity gate |
| 2 | `make test CRATES="quick-tunnel"` | the unit tests of the named crates |
| 3 | `make test` | every unit test of the workspace, doc tests included |
| 4 | `make e2e-tui`, `make e2e-explore`, `make vision` | the pane, the Explore page, the real UI |

`make check` runs levels 1 to 4 but `make vision`. No check fails on formatting:
run `make fmt` once a feature is complete. The test summary names each test
slower than 10 s, and `target/nextest/default/junit.xml` keeps every test's time.

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

### Parallel workspaces

`/tmp` may be a RAM disk, and a full build needs about 16 GB. Put a second jj
workspace or checkout on disk, or point `CARGO_TARGET_DIR` at the main
checkout's `target/`, before running `make check` in it.

A jj workspace has no `.git`, so `nix develop` there copies the whole directory,
`target/` included, into the Nix store. Enter the dev shell through
`scripts/dev-shell`, which hands Nix a copy of `flake.nix` and `flake.lock`
alone, a changed flake included:

```sh
scripts/dev-shell make check
```

Work in another workspace must stay visible to the user, who follows it from
the main checkout with `jj log` and `jj diff -r <bookmark>`:

- The supervisor names each agent's workspace and bookmark when it starts it.
- The agent describes its change at the start, with a provisional subject.
- The agent runs `jj st` after each batch of edits: jj records a workspace's
  files only when a jj command runs there.
- The agent closes each slice that works with `jj new`, so progress shows as
  described commits.

The Herdr integration tests copy their test binary, about 400 MB, into a
private directory under `/tmp` for each test. When several gates run at once, a
test that talks to the Herdr socket can fail with `WouldBlock`, and pass when
run alone.

## Explore page

The Explore page is a browser page that shows an Explore round. Its routes,
socket and client are in
[`crates/review-explore-page`](../crates/review-explore-page): axum serves a shell
page that loads a small JavaScript client (`assets/client`, see
[The page's client](#the-pages-client)), with no framework, no bundler and no
package manager. Each open page holds one WebSocket at `/ws`: the tool sends the round
as typed data (`PageView` in `src/view.rs`), whole, when the socket opens and at each
change, and the client draws every screen from it and changes in place what changed,
so the reader's scroll, focus, selection and typed text stay. The reviewer's actions go
back over the same socket as requests in the shape of JSON-RPC 2.0 (`src/rpc.rs`); the
page hands each one to the round's owner as a `PageCommand`, waits for its reply, sends
the view it changed, then the reply. A refusal carries its notice, a status card
worded for the action by `StatusCard` in `src/status.rs`, which the page shows until
the round changes. Before it hands an action to the owner, the page checks that the
round still offers it, by the identity of what the action acts on: the version of the
question, the start, the turn and its latest attempt, the answer, the round, the
conclusion, the implementation request and its attempt. The owner checks again against
its own round. A repeat of an action that went through (the same answer to the same
version of a question, the same first pick, list to be implemented or quiz answer) is
answered as applied already and changes nothing; a repeat of a Start, a Stop
waiting, a Retry or a resend names a start, a turn or an attempt that is no longer the
current one, so it cannot act twice (`CommandRefusal::AlreadyApplied`,
`src/actions.rs`). With scripts off, the page says that it needs them.

On a question whose Door is not two-way, the page hides the recommendation until the
reviewer's first pick (`src/blind.rs`), unless the reviewer answered the question
before and cancelled the answer (`RoundStage::Question::answer_cancelled`): the view the
page holds before that pick carries neither the agent's reason nor any mark of the
recommended choice, and the choices come in a mixed order. The reviewer's first Send is
the pick, and sends no answer: a request that the owner accepts while the round asks the
question (`PageCommand::Pick`), after which the page keeps it (`FirstPicks`) and shows
every tab the recommendation, with the line that says whether the reviewer and the agent
picked the same choice; the button becomes Confirm answer, and the answer it sends carries
the first pick to the owner as `AnswerInput::first_pick`. The comment typed before the
first Send stays in the answer's comment box, which keeps the same draft. A conclusion with a quiz shows it
first, one item at a time (`assets/client/quiz.js`): the page grades a pick itself,
saves it through the owner (`PageCommand::Quiz`), then shows the item's answer until the
reviewer moves on; once every item is answered or the reviewer skips the rest, the
conclusion shows with the results folded beside it.

The page offers every action of the pane's Explore tab for the round's state,
through `PageCommand`, and the session carries each one out by the path of the
pane's command, so that it saves the same result and sends the same prompt
(`crates/review-explore-session/src/page_actions.rs`). The pane follows each one
through the events it already follows (`ExplorePosted`, `ExploreAnswerCancelled`,
`ExploreImplementationSaved`, `ExploreImplementationFinished`), and through
`ExplorePageStopped` and `ExplorePageReset`; its own behaviour does not change.
On the network, Reset ends the round's token, and the page that sent it receives
the start screen's next token in the reply (`Rounds::after_reset`, ADR 0003), and opens
again with it.

| State of the pane's Explore tab | Pane action | On the page (`RoundStage`) |
| --- | --- | --- |
| No round | Start, Start with Challenger | Start, Start with Challenger (`NoRound`) |
| The latest start failed | Retry, which starts again | The reason, and Start again (`StartFailed`) |
| Capturing the change, waiting for a start from the page, or a kickoff waiting for Jev | Stop waiting | Stop waiting (`Starting`) |
| Stopped while capturing | None: waits for the capture to end | `Starting`, then `NoRound` |
| Waiting for the agent | Stop waiting; Cancel answer on the latest answer | Stop waiting; Cancel this answer, in the panel of the answer the turn carries (`AgentWorking`) |
| Interrupted: the prompt failed, the agent did not start on it, the reviewer stopped waiting, or reopened during the turn | Retry, with the reason; Cancel answer | Retry, with the failure, the agent not starting, an unknown delivery or a stop (`Interrupted`), in the panel of the answer the turn carries when it carries one; Cancel this answer |
| Interrupted with no turn to send again | Reset | That only Reset is left (`Interrupted` with no request) |
| A question | Send; Cancel answer of the previous answer | Send answer; on a blind question, a first Send that shows the recommendation, then Confirm answer (`Question`); Cancel this answer |
| An earlier question in the history | A free-text answer | None: the question opens read only from its step on the rail (`#question-N`), with the answer and what the agent recorded |
| The conclusion | Implement; Reply; Cancel answer until a request is made | Implement; "Not ready? Reply to the agent instead", which opens the chat, whose message answers nothing; Cancel answer (`Conclusion`), after the quiz, which only the page asks |
| An implementation request being sent | Cancel implementation | Cancel the implementation request |
| A request saved but not sent, by an earlier process | Send saved implementation request; New implementation request | Send the saved request; Edit before sending, then Send a new request |
| A request whose delivery is unknown | New implementation request | Send a new request anyway, after the warning |
| A request the agent did not start on | Retry; New implementation request | Retry only: the list may still wait in the agent's prompt box |
| A request that was not sent or was cancelled | Implement | Implement |
| A request the agent received | None | Start a new round… (Reset, behind Confirm: start a new round); Reply to the agent, which opens the chat |
| Any running round | Reset, then Confirm reset | Reset, then Confirm reset, in the masthead's ⋯ menu |
| An earlier round, or one whose history was repaired | Reset only | That it can no longer change, and Reset only (`earlier`) |
| A storage error | None: the status says why | Why, and to reopen the pane once fixed (`StorageFailed`) |
| Any | History navigation, the provisional map, marks lists, evidence windows | The design screen (`#design`) and each earlier question, read only (`#question-N`), for history; what an answer marks in its gain line, and the marks of each file in the meter's window; the current stage's citations; no provisional map |

The agent's Markdown (the design explanation, a question's Context, Door and
Blast radius, the conclusion) is rendered to HTML on the server by
[`crates/markdown-html`](../crates/markdown-html), which shows raw HTML as text
and keeps a fenced block's language as the class `language-<name>` of its
`<code>`. Callouts (`> [!TIP]`) and table-cell status marks (`[!good]`) are
defined once in [`crates/markdown-marks`](../crates/markdown-marks): the page,
the pane's renderer and the kickoff prompt all read them from there.

The page follows the reviewer's design handoff in
[`docs/design/explore-page/`](design/explore-page/README.md): its `README.md` is the
specification, `screenshots/` the reference captures, and `design-review.md` the detailed
findings; "The page's components" below says where each building block lives.

A fenced `mermaid` block is a diagram, which `assets/client/diagrams.js` draws in the
browser with Mermaid when the region that holds it is built, every diagram of the page the
same way (design review, finding 30): in the page's theme and font, with colours read from the
tokens at each draw and again when the page turns dark or light; whole, as the project owner
asked: at its natural size when it fits its frame, otherwise shrunk to fit and never enlarged;
when that would take its 14-pixel text under 11 pixels, the figure first widens to the whole
reading column (class `wide`), and past that a desktop still shrinks it to fit while a phone
keeps its natural size in a frame that scrolls sideways and says so; a shrunk or scrolling
diagram offers "Open large", a modal dialog at its natural size that scrolls and closes with
Escape; a `flowchart LR` that does not fit drawn again top to bottom; and the nodes classed `new` or `changed` in green and amber, with a legend. The
kickoff's diagram rules (`crates/review-explore-runner/src/diagrams.md`) name that vocabulary.
Each table of the agent's Markdown sits in a frame of its own that scrolls sideways, with its
first column held on a phone. Mermaid is
vendored, pinned and gzipped at build time in
[`crates/mermaid-js`](../crates/mermaid-js) (its `vendor/README.md` says how to
move to another version); the page serves it itself, loads it only when it
shows a diagram, and lets the browser keep it, since its address names its
version. Mermaid writes inline styles into each diagram, so the page's content
security policy allows inline styles (`style-src 'self' 'unsafe-inline'`); it
still allows scripts only from the page itself. The tool cannot check a diagram
when the agent submits it: when Mermaid cannot parse one, the page shows its
source under a quiet caption, with Mermaid's message behind a fold (design review, finding 22),
and sends the error over the socket (`diagram-failed`), and
the Explore session saves it with the question (`Exploration::diagram_errors`).

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
the page of that round on a loopback port behind a token (below). The worker names the
review the page belongs to after each snapshot (`ExploreSession::name_review`,
`RoundPublisher::name`), and the start screen shows it. Whether a round can start follows
one rule, `review_explore::StartBlock`: not once every changed line is marked as reviewed.
The worker hands the session the review states after each snapshot, each mark the reviewer
sets, and each Explore input that ends a round (`ExploreSession::marks_changed`); the
session tells the pane (`ExploreStartBlock`) and the page (`RoundPublisher::block_starts`),
which show the start buttons inactive with the reason, and it reads the marks again as it
captures the change and before it sends a kickoff, so a start that finds nothing left to
review fails without one. The page's
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
`xdg-open` or `open`. The record keeps the page's token and the review it shows after
the reviewer closes: a reviewer of the same review that starts again in the workspace
serves the page at the same address with the same token, when its port is free, so that a
page left open reconnects to it.

The page also shows the round's conversation with the agent, the chat: a review thread
attached to the round (`Post::to_round` in `crates/review-threads`, read through
`review_round_conversation::RoundConversation`), never an Explore turn. The page reads it from
the review threads, which their owner publishes (`ThreadsPublisher`, `ThreadsFeed`), and writes
it with the pane's thread commands (`ThreadSender`): `PageRound::with_conversation` gives a
page both (`PageConversation`). In the reviewer, the thread worker's events publish the threads and the
latest wakeup of each review, and a post from the page gets its reply once the worker reports
it (`crates/reviewer/src/runtime/page_threads.rs`); the round names its review
(`PublishedRound::review_unit`), whose threads hold its conversation. A message carries the
identity the page chose for it, so that a repeat posts it once; the place in the round the page
showed (`AskedUnder`: the question and its version, the design, the conclusion) and the
passage it quotes; it wakes the agent with the threads' usual prompt, which names the round
and the question and says that it is no answer. The view's `conversation` holds the messages
with their Markdown rendered and their place named as the rail names it, the unread count, and
a status card with Retry when a wakeup did not reach the agent.

The host also serves the page on a network interface, for a phone, on the same
thread and runtime: a second listener with its own host name and a new token for
each round (`PageHost::share`). While no round runs, the listener has a token for
the start screen, which the round started next keeps, so the page that started
it stays connected. It announces the page's address to the pane each time the
token or its round changes, and the pane draws it with its QR code
([`crates/ui-qr-code`](../crates/ui-qr-code)). When the listener cannot start, the pane
says why in place of the address (`ExplorePageNotShared`), with no toast. The settings
are the reviewer's Explore page settings (`ExplorePageSettings` in
[`crates/review-explore-page-settings`](../crates/review-explore-page-settings), saved by
`ReviewStore` in `settings.json`; [usage guide](usage.md#explore-page-settings)). The pane
changes one at a time and sends it as `SettingsAction::SaveExplorePage(ExplorePageSetting)`;
the runtime saves that setting alone (`ReviewStore::save_explore_page_setting`, which keeps
what another reviewer saved), sends the pane the settings as saved
(`ExplorePageSettingsLoaded`), and `PageSharing` applies their network part at once: it moves the page to a new listener
(`PageNetwork::share`) or takes it off the network (`PageNetwork::unshare`, then
`ExplorePageOffNetwork`, after which no address of the old listener reaches the pane). Herdr
test servers write a `settings.json` that turns network access off into their private state
directory (`HerdrTestServer`); a `make vision` session sets the loopback interface and any free
port instead (`HerdrTestServer::set_explore_page_settings`), so the pane shows a QR code that
only this machine can open. Test sessions open no browser: they set `BROWSER` to a stand-in.

The pane's `O` shares the running round over a Cloudflare quick tunnel
([ADR 0004](adr/0004-share-a-round-over-a-cloudflare-quick-tunnel.md)):
`ExplorePageAction::OpenTunnel` and `CloseTunnel` reach `PageSharing`, which calls
`PageNetwork::open_tunnel` and `close_tunnel`; the pane hears `ExplorePageTunnel` with a
`TunnelState` ([`crates/review-explore-page-tunnel`](../crates/review-explore-page-tunnel)):
opening, open with the link, failed with one line, or off. The tunnel's process lives in
[`crates/quick-tunnel`](../crates/quick-tunnel): `QuickTunnel` runs `cloudflared tunnel
--no-autoupdate --url http://127.0.0.1:<port>` as an `agent-fork` fork, through
`reviewer-control fork-exec`, reads the `….trycloudflare.com` host name from its log on the
standard error (`ForkOutput::error_line`), and gives up after 30 seconds. The host
(`crates/review-explore-page-host/src/tunnel.rs`) binds the loopback listener the tunnel
forwards to, and serves the page there only once the host name is known, with `Hosts::tunnel`,
which admits that name with an `https://` origin and a `wss:` socket; its rounds
(`TunnelRounds`) are the network listener's `RoundTokens` limited to the round the tunnel
shares, and a Reset from the tunnel's page hands it no token. The network listener and the
tunnel share the tokens, made when either starts and closed when both stopped. A task stops the
tunnel when the published round changes; a tunnel is stopped on a thread of its own, which the
page host waits for when it drops. Tests use a stand-in script for `cloudflared` that prints the
address as the real one does; requests reach the tunnel's listener directly, with the headers
`cloudflared` forwards (the public `Host`, an `https://` `Origin`).

### The page's components

Each component is plain markup with a few classes, styled in one stylesheet of
`crates/review-explore-page/assets`, which the client's modules build with the same
classes. Colours, type, radii and shadows come from the tokens in `tokens.css`
(one meaning per colour: accent where the reviewer acts or what is selected, good for done,
warn for "check before you act", bad for failed or destructive, agent for the agent's
judgement); `h1` to `h4` follow the type scale, `.eyebrow` is the small uppercase label above
what it names, `.hint` the muted help line. Every control has a focus ring (`:focus-visible`).

- **Buttons** (`buttons.css`): `<button class="button primary">`, with one tier among
  `primary` (the one thing to do in the view), `secondary`, `outline`, `quiet` (underlined
  text) and `danger` (only Confirm reset); add `block` for the full width of a panel. A link
  may carry the same classes.
- **Disclosure** (`client/disclosure.js`, styled in `buttons.css`): a button with
  `aria-expanded` that shows or hides an action behind a fold, a muted line with its ▸ by
  default (`<button class="disclosure-toggle" aria-expanded="false">`), or a button of any
  tier (the conclusion's "Start a new round…"). A button rather than `<details>`, so that
  readers and the e2e agent see a control.
- **Status card** (`status.css`), for every state that is not a question:

  ```html
  <div class="status-card warn" id="interruption" role="alert" aria-labelledby="interruption-title">
    <span class="status-glyph" aria-hidden="true"></span>
    <p class="status-title" id="interruption-title">The agent may or may not have your answer</p>
    <p class="status-reason">The review pane was reopened while it sent the prompt.</p>
    <div class="status-bar" aria-hidden="true"></div>   <!-- progress only -->
    <p class="status-next"><strong>Check the agent's pane before you retry.</strong></p>
    <div class="status-actions">
      <form class="retry" data-method="retry">
        <input type="hidden" name="request" value="…">
        <button class="button secondary" type="submit">Retry</button>
        <p class="hint">Sends the same turn again, which could duplicate it.</p>
      </form>
    </div>
  </div>
  ```

  The kind is one class among `progress`, `info`, `warn`, `danger` and `ok`; the glyph comes
  from the stylesheet. A verbatim error in the reason is a `<code>`. Which state shows which
  card, with its words and actions, is data: `StatusCard` in
  [`src/status.rs`](../crates/review-explore-page/src/status.rs) maps each stage, each
  implementation request and each refused action to its kind, title, reason, next step and
  actions; `assets/client/status.js` only draws it. In a panel, where the primary action
  comes last at its full width, `panelCard` draws the card without its actions and
  `panelActions` draws them as the panel's buttons (the conclusion's implementation request,
  the answer the agent's turn carries). A card that waits says the time since what it waits for
  began (`since`: "Sent 0:42 ago"), which the page counts on every second, in tabular numerals
  and quietly for a screen reader; the tool says when (the start, or the turn's latest attempt
  going out), the page counts.
- **The answer the agent's turn carries** (`sent.css`, `client/sent.js`, from the view's
  `sent`, `SentView` in `src/view/sent.rs`): while the agent works on a turn that carries the
  reviewer's answer, and when that turn did not go through, the turn's status card sits on a
  desk of its own (not above the stage), then the answered question, read again as it read
  before the answer, at full contrast (`questionReading`, "Question 2 · answered"; its
  citations folded or open as the reviewer left them on the question, kept by `citations.js`;
  the project owner's request, where the handoff's capture mutes it); the panel holds the
  question's choices, read only with the one sent selected (`choiceCards(..., { answered })`,
  `QuestionView::keeping`), the comment and tags and what the answer covered (`answer-card.js`,
  `answer-card.css`, as an earlier question's panel shows them), the card's one
  action (Stop waiting, or Retry) and Cancel this answer. The panel comes right after the card
  in the markup, so a phone shows the answer and its action before the question. A turn that
  carries no answer (the kickoff) shows its card above the stage, as before:

  ```html
  <div class="sent desk">
    <div class="status-card progress" id="waiting" role="status">…Sent <span class="status-elapsed">0:42</span> ago…</div>
    <section class="sent-answer panel" aria-labelledby="sent-answer-title">
      <p class="eyebrow" id="sent-answer-title">Your answer to question 2</p>
      <fieldset class="choices answered" disabled><legend class="sr-only">Choices</legend><label class="choice">…<input type="radio" checked>…</label>…</fieldset>
      <div class="answer-card"><p class="answer-comment">“…”</p><p class="answer-tags"><span class="tag accent">changed after your first pick</span></p></div>
      <p class="answer-marked"><span class="check">✓</span> Marked 15 lines reviewed</p>
      <form class="stop" data-method="stop">…<button class="button secondary block">Stop waiting</button><p class="hint">…</p></form>
      <div class="disclosure">…Cancel this answer…</div>
    </section>
    <section class="answered-question" aria-labelledby="question-2-label">…</section>
    <section class="citations" aria-labelledby="question-2-citations">…▸ 1 more citation…</section>
  </div>
  ```
- **Panel and desk** (`layout.css`): the page is capped at 90rem with a 32-pixel gutter (16 on
  a phone). From 70rem, a container with the class `desk` reads in two columns: its children
  in the reading column, each in its own grid row, and its child with the class `panel` (416
  pixels; 480 with `desk wide`) beside them from the first row to the last, sticking in view.
  The panel is a tinted surface with no border; on a phone it runs from edge to edge. From
  89rem (1424 pixels), the open chat stands as a third column at the window's left gutter, and
  the page starts after it (`--chat-room`): the reading column narrows, down to 448 pixels
  (384 beside the conclusion's wider panel), and the panel keeps its width and its place;
  between 70 and 89rem the chat lies over the left of the page and nothing moves. Because
  the grid places the parts, a template keeps one order, which a phone shows as is: a reading
  part after the panel (the question's citations) comes after the reviewer's actions there.

  ```html
  <section class="question desk" aria-labelledby="question-1-title">
    <h2 id="question-1-title">Question 1</h2>
    <div class="text markdown">…</div>
    <form class="answer panel" data-method="answer">…</form>
    <section class="citations">…</section>
  </section>
  ```
- **Start cover** (`style.css`, "start cover"): the status cards come first when a start
  failed or nothing is left to review, then

  ```html
  <section class="start-cover">
    <p class="eyebrow" role="status">No round is running</p>   <!-- only without a card -->
    <h1>The review's title</h1>
    <p class="meta"><code>revision</code> in <code>repository</code> · +125 −10 in 4 files</p>
    <form class="start" data-method="start">
      <div class="start-choice">
        <button class="button primary block" type="submit">Start</button>
        <p class="hint">The agent explains the design, then asks one question at a time.</p>
      </div>
      <div class="start-choice">…Start with Challenger…</div>
    </form>
  </section>
  ```
- **Masthead** (`masthead.css`, drawn by `assets/client/masthead.js`), above `main`: the
  chat's place, the product's name and the review's title, the round rail and the ⋯ menu, with
  its hairline across the window as an element of its own, which the meter draws on. The rail
  and the tab title come from the round's overview (`review_explore::RoundOverview`, which the
  session derives in `publish_page` and the view carries as `rail` and `title`): take every
  question number from it, never from a count of the question's versions. "Design ▾" opens the
  design map in place; the menu copies the page's address, opens the agent's conversation, and
  holds Reset, which the page offers nowhere else. Its map links to the design screen
  (`#design`, `#design-part-N`), and while that screen shows, the rail shows Design as current
  (`aria-current="page"`) and the round's own step as the next one (class `next`), which keeps
  `aria-current="step"`. Each done
  question is a link to its earlier question (`#question-N`), which shows the same way; the
  round's own step is then a link back to the stage (`#round`), and a step that is a link
  carries its `aria-current` on the link. Each step names the screen it leads to in
  `data-screen` (`design`, `question-N`, `round`), for the swipe. On a
  phone the rail shows the design and the current step as chips, with the screen in view and
  the screens beside it, which a swipe turns to (class `near`), and the chip a swipe heads to
  fills (class `target`); the review's title moves into the menu. The chat's place is first in
  the row from 70rem, where the bubble stands at the window's left gutter, above the chat it
  opens, and the product's name begins after it (or, with the chat open beside the page, where
  the reading column begins); below 70rem `masthead.js` moves it beside the menu, so that the
  keyboard meets the bubble where the row shows it.

  ```html
  <header class="masthead">
    <div class="masthead-chat" id="masthead-chat">          <!-- the chat's bubble; beside the menu below 70rem -->
      <button class="chat-bubble" aria-label="Talk to the agent" aria-controls="chat"><span class="bubble-shape"></span><span class="chat-badge">1</span></button></div>
    <div class="identity"><p class="product">Explore</p><p class="review">…</p></div>
    <nav class="rail" aria-label="Round"><ol>
      <li class="step done design-step"><button class="design-toggle" aria-expanded="false">…</button>
        <div class="design-map" id="design-map" hidden>…</div></li>
      <li class="step current" aria-current="step">Q2 · working</li>
      <li class="step later">Quiz</li>
    </ol></nav>
    <div class="menu"><button class="menu-toggle" aria-label="Round menu">⋯</button>
      <div class="menu-popover" id="round-menu" hidden>…</div></div>
    <div class="masthead-line" id="masthead-line"></div>   <!-- the meter draws here -->
  </header>
  ```
- **Chat** (`chat.css`, `client/chat.js`, `client/chat-quote.js`): the round's conversation
  with the agent, after `main`. On a desktop (from 70rem) it stands at the window's left gutter,
  under its bubble, as wide as the panel and drawn as the panel is; it starts under the
  masthead, rises with it to the panel's height as the page scrolls (a scroll timeline, or
  `rise()` in a browser without one), and
  reaches the bottom of the window, its messages scrolling on their own above the composer.
  From 89rem the page makes room for it (`layout.css`, "Panel and desk"); between 70 and 89rem
  it lies over the left of the page, with a shadow and no scrim; it never covers the panel. It
  sits under the masthead's layer (`--layer-chat-beside`), so the design map, the ⋯ menu and
  the meter's window open over it, and an Escape that closes the map, the menu, the meter's
  pinned window or a diagram opened large leaves the chat open (the chat hears Escape on the
  window, after every listener of the document). Below the desk's two columns it is a bottom sheet over the
  dimmed page, whose grabber drags it to the full height or closes it. It opens from the bubble, from the menu,
  from "Not ready? Reply to the agent instead" (`requestChat()`), and from "Add to chat", which
  a passage selected in the reading offers and which it quotes. The composer is a form of
  `actions.js` (`send-message`); its draft and quote are kept for the tab. A reply the reviewer
  has not seen counts on the bubble and in the tab's title until the chat shows it, which marks
  it read (`read-messages`):

  ```html
  <aside class="chat open" id="chat" aria-label="Conversation with the agent">
    <header class="chat-head"><h2 class="chat-title">Agent</h2><span class="chat-meta">this round · 3 messages</span><button class="chat-close" aria-label="Close the conversation">×</button></header>
    <div class="chat-log" role="log" aria-label="Messages">
      <article class="chat-message mine" aria-label="Your message"><p class="eyebrow">You · Q2 · 14:21</p><blockquote class="chat-quote-shown">…</blockquote><div class="markdown chat-text-shown">…</div></article>
      <article class="chat-message agent" aria-label="Reply from the agent"><p class="eyebrow">Agent · 14:22</p><div class="markdown chat-text-shown">…</div></article>
      <p class="chat-pending" role="status"><span class="chat-pending-glyph">…</span><span>The agent is answering · <span class="chat-pending-time">0:31</span></span></p>
      <div class="status-card danger" id="conversation-delivery" role="alert">…Retry…</div>
    </div>
    <form class="chat-composer" data-method="send-message" data-requires="text">…<div class="chat-quote">…</div><textarea class="chat-text" aria-label="Message to the agent"></textarea><div class="chat-send"><button class="button outline" type="submit">Send</button><span class="hint">The question stays open. ⌘↵</span></div></form>
  </aside>
  ```
- **Design screen** (`design.css`): the design of the change on the desk, its map in the panel.
  The part in view carries the class `current` (and its map link `aria-current="location"`),
  the parts above it `read`, and the section says its number in `data-part`. On a phone the map
  sits under the thesis, part headers stick to the top, and `.design-bar` is pinned to the
  bottom.

  ```html
  <section class="design-screen desk" aria-label="Design of the change" data-part="1">
    <header class="design-head"><p class="eyebrow">…</p><h2>The change's thesis</h2><p class="meta">4 parts · about 4 minutes · 3 files</p></header>
    <nav class="design-nav panel" aria-label="Design map">… <a class="button primary block" href="#round">Go to question 1</a></nav>
    <section class="design-part" id="design-part-1" aria-labelledby="design-part-1-title">
      <header class="part-head"><span class="n">1</span><h3 class="eyebrow" id="design-part-1-title">What it adds and where</h3><span class="part-of">1 / 4</span></header>
      <p class="thesis">…</p>
      <div class="markdown">…</div>
    </section>
    <div class="design-bar"><p>Current · Question 1</p><a class="button primary block" href="#round">Go to question 1</a></div>
  </section>
  ```
- **Earlier question** (`earlier.css`, `client/earlier.js`, `earlierScreen(question,
  current)`): a question the reviewer answered before, from `PageView.earlier_questions`
  (the overview's `earlier` records, with their citations resolved by the session): on the desk
  the question head ("Question 1 · answered" and its Door chip), Context, the Door and Blast
  radius rows and the citations, as the question screen draws them; in the panel the kept
  answer in a card with its decision tags and what the answers marked (`answer-card.js`), what the agent recorded
  (the previous turn's `recorded` block) and the way back to the round's step (`goTo` of
  `design.js`). It holds no form: nothing on it can change the round.

  ```html
  <section class="earlier-question desk" aria-labelledby="earlier-1-label">
    <header class="question-head"><p class="eyebrow question-eyebrow"><span id="earlier-1-label">Question 1 · answered</span> <span class="chip good">Two-way door</span></p><h2 class="question-text">…</h2></header>
    <section class="earlier-panel panel" aria-labelledby="earlier-1-answer">
      <p class="eyebrow" id="earlier-1-answer">Your answer to question 1</p>
      <div class="answer-card"><p class="answer-choice">…</p><p class="answer-comment">“…”</p><p class="answer-tags"><span class="tag agent">as recommended</span></p></div>
      <p class="answer-marked"><span class="check">✓</span> Marked 12 lines reviewed · 3 lines not relevant</p>
      <div class="earlier-record"><section class="turn-record">…</section><p class="earlier-follow-ups">Follow-ups · …</p></div>
      <a class="button primary block" href="#round">Go to question 3 →</a>
    </section>
    <div class="markdown explanation">…</div>
    <div class="assessments">…</div>
    <section class="citations">…</section>
  </section>
  ```
- **Chips and tags** (`tags.css`): a pill of 12 pixels. A chip names what a thing is, outlined
  in its colour; a tag marks one item of a list, on a tint of its colour. One colour among
  `good`, `warn`, `bad`, `agent`, `accent` and `neutral`. The chip of a question's Door comes
  from `doorChip(door)` in `client/chips.js`, on the question screen and in the design
  screen's panel:

  ```html
  <span class="chip warn">One-way door</span>   <span class="chip agent">Blind pick</span>
  <span class="tag agent">◆ Agent recommends</span>   <span class="tag accent">Your first pick</span>
  ```
- **Previous turn** (`turn.css`, `client/turn.js`, `turnStrip({ answer, response, number })`,
  with `number` the view's `answered`, the rail's number of the question the answer answered):
  one hairlined block, with no frame and no fill, above the stage the turn led to (the first
  row of the question's desk and of the conclusion's). While the agent works on the next turn,
  or that turn waits for Retry, the panel of the answer the turn carries shows the answer and
  Cancel this answer instead; the block sits above the stage only for a turn that carries no
  answer. What the reviewer answered beside what the agent recorded and
  replied, each a region named by its eyebrow; then the follow-ups and Cancel this answer, a
  disclosure (`disclosure.js`) with the quiet button's tier that opens its hint and Confirm:
  cancel my answer. On a phone the columns stack, each `turn-text` clamps to two lines, and the
  follow-ups hide:

  ```html
  <div class="turn">
    <section class="turn-answer" aria-labelledby="turn-answer-title">
      <p class="eyebrow" id="turn-answer-title">You answered Q1</p>
      <div class="turn-text"><p class="turn-choice">…</p><p class="turn-comment">“…”</p></div>
    </section>
    <section class="turn-record" aria-labelledby="turn-record-title">
      <p class="eyebrow" id="turn-record-title">The agent recorded</p>
      <div class="turn-text"><div class="markdown turn-recap">…</div><div class="markdown turn-reply">…</div></div>
    </section>
    <div class="turn-foot">
      <p class="turn-follow-ups">Follow-ups · … · …</p>
      <div class="disclosure">
        <button class="button quiet" type="button" aria-expanded="false" aria-controls="disclosure-1">Cancel this answer…</button>
        <div class="disclosure-body" id="disclosure-1" hidden>
          <form class="cancel-answer" data-method="cancel-answer">…hint…<button class="button secondary" type="submit">Confirm: cancel my answer</button></form>
        </div>
      </div>
    </div>
  </div>
  ```
- **Question head** (`question.css`): the eyebrow with the question's number, which names the
  question's region, its chips, and on a phone the link to the answer; then the question as the
  headline (`h2`, the agent's Markdown):

  ```html
  <header class="question-head">
    <p class="eyebrow question-eyebrow">
      <span id="question-2-label">Question 2</span>
      <span class="chip warn">One-way door</span> <span class="chip agent">Blind pick</span>
      <a class="choices-link" href="#answer">Choices ↓</a>
    </p>
    <h2 class="question-text">…</h2>
  </header>
  ```

  The door's chip: "One-way door" `warn`, "Two-way door" `good`, "Mixed door" and "Door
  unknown" `neutral`; "Blind pick" while the question is blind.
- **Door and Blast radius rows** (`question.css`, each a disclosure of `disclosure.js` whose
  button is a `disclosure-row`, a whole row after its ▸, in `buttons.css`): hairlined rows that
  open; folded, the lead
  (`SectionView::lead_html`, the section's decisive reason) follows the label on one line, cut
  with an ellipsis; open, it wraps and the rest (`details_html`) follows:

  ```html
  <div class="assessments">
    <div class="disclosure assessment">
      <button class="disclosure-row assessment-toggle" type="button" aria-expanded="false" aria-controls="disclosure-2">
        <span class="assessment-title">Door</span><span class="assessment-lead"><p>…</p></span>
      </button>
      <div class="disclosure-body" id="disclosure-2" hidden><div class="markdown">…</div></div>
    </div>
  </div>
  ```
- **Code frame** (`citations.css`, `client/citations.js`, `citation(view, id)`): the note, then
  a `--panel` frame whose head (the citation's heading, which names its region) gives the path
  and the lines; added and removed rows carry a bar in the gutter, and their tint unless every
  row is added (`all-added`). A phone shows one number column, the new line's or, on a removed
  row, the old one's. Citations after the first wait behind a disclosure ("▸ 1 more citation").
  The quiz's proofs use the same frame:

  ```html
  <section class="citation" aria-labelledby="question-2-citation-1">
    <p class="notes">…</p>
    <div class="code-frame">
      <h4 class="code-head" id="question-2-citation-1"><code>src/threads/reply.rs</code> <span>new 20-27</span></h4>
      <div class="code" tabindex="0" role="group" aria-label="Lines of …">
        <table><tbody><tr class="added"><td class="number old"></td><td class="number new">20</td><td class="sign">+</td><td class="line">…</td></tr></tbody></table>
      </div>
    </div>
  </section>
  ```
- **Choice card** (`choices.css`, `choiceCards(choices, { name, legend, picking, revealed })`
  in `client/choices.js`, for a question's choices and a quiz item's answers): a card on the page's surface with an 18-pixel radio on its
  first line. Hover strengthens its frame (`--line-strong`); the selected card takes the
  accent frame, 2 pixels, and an accent tint; the keyboard's focus rings it with a 2-pixel
  outline. The radio is named by `choice-text` alone. A recommended card keeps a neutral frame
  and adds the **recommendation tag**, a `choice-tags` row with the purple tag, and the
  agent's reason under a dashed hairline, which describes the radio; the reviewer's first pick
  of a blind question keeps the accent tag "Your first pick" once the recommendation shows:

  ```html
  <fieldset class="choices">
    <legend class="eyebrow">Choices</legend>
    <label class="choice recommended">
      <input type="radio" name="choice" value="…" aria-labelledby="choice-3" aria-describedby="recommendation-3">
      <span class="choice-text" id="choice-3">…</span>
      <span class="choice-tags"><span class="tag agent">◆ Agent recommends</span></span>
      <span class="choice-reason" id="recommendation-3">…</span>
    </label>
  </fieldset>
  ```
- **Reveal line** (`question.css`, `revealLine(choices)` in `client/question.js`): at the top of the answer panel once the first Send of a
  blind question showed the recommendation; purple when the agent recommends another choice,
  green when both picked the same. Never warn or red: disagreeing is information.

  ```html
  <p class="reveal other" tabindex="-1" data-shows="pick"><strong>The agent recommends another choice.</strong> Read its reason, then keep yours or change it.</p>
  <p class="reveal same" tabindex="-1" data-shows="pick"><strong>You and the agent picked the same choice.</strong></p>
  ```
- **Gain line** (`question.css`, `gainLine(marks, gain)` in `client/question.js`): what answering marks (`MarksView::summary`, a
  `MarkPhrase`: its verb, then each amount, the first in bold) and the reviewed share of the
  change before and after (`GainView`, from the mark tally's `Gain`, rounded as the reviewer's
  file list rounds it), with a bar: reviewed in `--good`, what the answer adds hatched in
  accent. The lines open on request: the whole block is a disclosure's button. Without a share
  (the tool cannot tell), the line shows the amounts alone:

  ```html
  <div class="disclosure gain">
    <button class="disclosure-row gain-toggle" type="button" aria-expanded="false" aria-controls="disclosure-3">
      <span class="gain-line">
        <span class="gain-text">Answering marks <strong>12 lines reviewed</strong> · 3 lines not relevant</span>
        <span class="gain-share">38% → <strong>49%</strong></span>
      </span>
      <span class="gain-bar" aria-hidden="true"><span class="gain-done" style="width: 38%"></span><span class="gain-added" style="width: 11%"></span></span>
    </button>
    <div class="disclosure-body" id="disclosure-3" hidden>
      <ul class="gain-lines"><li>src/threads/reply.rs new 16-27 (reviewed)</li></ul>
    </div>
  </div>
  ```

  The answer panel holds, in order: the blind hint or the reveal line, the choices, the
  comment (`COMMENT · optional`, a box that grows with its text), the gain line, and Send
  answer or Confirm answer, which stays in view at the panel's bottom when a short window makes
  the panel scroll. A form may say what it needs before it can be sent with `data-requires`
  (`choice`, `choice-or-comment`, `answer`; `REQUIRES` in `client/actions.js`): its button stays dimmed
  until then. After an action, the page brings the part it changed into view when the reviewer
  cannot see it (`CHANGED` in `client/actions.js`): the element its module marks with
  `data-shows="<method>"` (the reveal line after a first pick, the verdict on a quiz answer,
  the question after Cancel answer), or the status card after an answer.
- **Quiz** (`quiz.css`, `QuizScreen` in `client/quiz.js`): one item at a time on a desk. The
  head is "QUIZ · QUESTION 2 OF 3" with a dot for each item (named in words for a screen
  reader) and the item as the headline; before Check the panel holds the answers as choice
  cards (`choiceCards`, `data-requires="answer"`) and Check, with a quiet Skip the quiz. After Check the panel
  opens with the verdict (`role="status"`, brought into view), then the answers with their
  marks, each a glyph from the stylesheet and a tag in words (✓ `Correct`, ✗ `Your pick`), then
  Next question (Show the conclusion after the last item); an answer after Check is a choice
  card with its mark in the radio's place. The proof joins the reading column.
  While the quiz shows an item, the page gives the masthead a rail whose quiz step is current
  at that item (`railShowing`), even right after the last one is checked. The results beside
  the conclusion (`quizResults`) reuse the verdict and the marked answers behind a fold, which
  the panel's "See the answers" opens (`openQuizResults`, through `openDisclosure` of
  `disclosure.js`):

  ```html
  <section class="quiz-panel panel" aria-label="Your answer to quiz question 2">
    <p class="verdict bad" role="status" tabindex="-1" data-shows="quiz"><span><strong>Not quite.</strong> …</span></p>
    <p class="eyebrow" id="quiz-answers-title">Answers</p>
    <ol class="quiz-answers" aria-labelledby="quiz-answers-title">
      <li class="choice quiz-answer wrong"><span class="mark" aria-hidden="true"></span><span class="choice-text">…</span>
        <span class="choice-tags"><span class="tag accent">Your pick</span></span></li>
    </ol>
    <button class="button primary block" type="button">Next question <span aria-hidden="true">→</span></button>
  </section>
  ```
- **Meter** (`meter.css`, drawn by `assets/client/meter.js` on the masthead's hairline): how
  much of the change the review marks cover, from `PageView.tally`
  (`review_explore_tally::MarkTally`). The session publishes the tally with each stage, in the
  same change (`RoundPublisher::publish_counted` in `publish_page`), and again from
  `marks_changed`, so a mark by hand or a run of Jev during a round reaches the page at once
  (`RoundPublisher::tally`). At rest the bar is the marked share
  in green; hovered, focused or open it grows and splits into the reviewer's answers, marks by
  hand or from earlier rounds, Jev and not relevant, and what the waiting question marks, and a
  window gives the totals, a legend and a row for each file. The window is as wide as its
  longest path needs, from the handoff's 520px up to 880px, within the viewport's gutters and
  never over the panel beside the reading column; it keeps the width it opened with while it
  stays open. A path that does not fit loses its start to an ellipsis, so that the file's name
  stays (the path sits in a left-to-right `<bdi>`, so that a leading `.` stays in place); each
  row is a list item named by its full path, which is also its title. Past the viewport's
  height the list of files scrolls under the totals and the legend. The strip is a button whose name
  carries the share for a screen reader; Enter or a click pins the window, Escape closes it.
  The start cover takes the change's size from the same tally (`change-size.js`).

  ```html
  <div class="meter open grown">
    <div class="meter-bar" aria-hidden="true"><span class="meter-segment answers"></span>…</div>
    <button class="meter-strip" aria-label="Lines reviewed: 38%, 52 of 135 changed lines"
            aria-expanded="true" aria-controls="meter-window"></button>
    <div class="meter-window" id="meter-window" role="group" aria-label="Review marks of the change">
      <div class="meter-totals">…</div>
      <div class="meter-legend">…</div>
      <div class="meter-files" role="list" aria-label="Files of the change">
        <div class="meter-file" role="listitem" title="src/notify/queue.rs" aria-label="src/notify/queue.rs">
          <span class="meter-path"><bdi dir="ltr"><span class="meter-dir">src/notify/</span><span class="meter-name">queue.rs</span></bdi></span>
          <span class="meter-file-bar">…</span>
          <span class="meter-file-state">18 left · <span class="cited">cited here</span></span>
        </div>
      </div>
    </div>
  </div>
  ```

### The page's client

The client is plain JavaScript ES modules in
[`crates/review-explore-page/assets/client`](../crates/review-explore-page/assets/client),
committed as they are: there is no build step, and the browser loads each module from
`/assets/client/<name>`. Every module must be listed in `ASSETS` in `src/files.rs`, which
builds it into the binary and serves it with the script content type browsers require
for modules. The page's content security policy allows scripts from the page itself
only, with no `unsafe` value; its `connect-src` names the page's own `ws:` address.

- `main.js` starts the client: it opens the socket and draws each view it receives.
- `socket.js` is the socket (`Link`): requests in the shape of JSON-RPC 2.0, each matched
  to its reply by an `id`; a new socket after a close, with a doubling delay capped at ten
  seconds and full jitter; a watchdog that opens a new one when the tool's pings (every ten
  seconds, `HEARTBEAT` in `src/socket.rs`) stop for 25 seconds; and an immediate check when
  the page is shown again, gets the focus, or the network comes back. Nothing is replayed
  after a reconnect: the new socket gets the current view, which the page draws whatever
  its number. While the socket is down, `connection.js` says so in a quiet line and every
  action waits, its button disabled; no action is queued. A socket closed with the code
  4001 had a token that no longer opens a round: the page says how to open it again.
- `dom.js` holds the rendering rules, the one place to read before changing a screen: one
  view, one entry point; stable regions, each rebuilt only when its key (its data) changes,
  so that the nodes the reviewer uses survive every push; text from the data through
  `textContent` (the helper `h`), and HTML only through `setRenderedMarkdown`, for the
  agent's Markdown the tool rendered, and `setDiagramDrawing`, for Mermaid's drawings; the
  agent's plain texts (a choice and its reason, a kept answer, a follow-up, a quiz item and
  its answers) through `codeSpans`, which draws their Markdown code spans as `<code>`
  elements, with no HTML; a text box's value set only when it is built, from
  its draft (`drafts.js`), and the focus given back to the text box of the same draft
  after a rebuild.
- `page.js` holds the page's screens, the design of the change, each earlier question and the
  round's current stage, lists the regions of each in the order the page shows them, and draws
  each from its part of the view. `route.js` says which screen the address shows: `#design` (or
  `#design-part-N`, 1 to 4) the design screen, which a link may target, `#question-N` earlier
  question N while the round has it, anything else the stage, which `#round` names. On a phone,
  `swipe.js` turns between the same screens, in the rail's order, with a sideways drag past 48
  pixels or a flick (README, "Swipe between screens"); a drag that starts in a frame that
  scrolls sideways, or in a text box, is left to it. The earlier questions' screens come after
  the stage, so the stage's diagrams keep their numbers; they are the one list of regions that
  grows with the round, one region for each earlier question, each rebuilt only when its data
  changes. The design opens the round: the first time a tab shows a round
  that waits for the answer to its first question, the page shows the design screen, and goes
  back to the stage once the round moves on unless the reviewer has navigated since. The page
  keeps the hidden screen's nodes, so the design's diagrams are the page's first. `actions.js` turns the submit of any form into its request:
  each form names its method (`data-method`), and `CALLS` says how its fields make the
  request's params.
- One module per screen or region: `start.js`, `status.js` (the status card),
  `design.js` (the design screen, with its map and the part in view), `earlier.js` (an earlier
  question, read only), `swipe.js` (the swipe between screens on a phone), `turn.js` (the
  previous turn), `sent.js` (the answer the agent's turn carries, beside the turn's card),
  `answer-card.js` (a kept or sent answer as a card, and what it marked), `chips.js` (the chip of a question's Door and the tags of a kept answer), `choices.js` (the choice cards), `question.js` (with the answer panel and the first pick), `citations.js`,
  `conclusion.js` (with the reviewer's decisions, the list to be implemented, each state of
  its request), `quiz.js`, `masthead.js` (above `main`, with Reset in its menu), `chat.js`
  (the chat, with its bubble in the masthead), `chat-quote.js` ("Add to chat" on a selection),
  `desk.js` (the windows where the page reads in two columns, for the chat and its bubble),
  `meter.js` (the meter on the masthead's hairline), `favicon.js` (the tab's icon, which shows
  the meter's share or the agent at work, with its timer in the worker `favicon-ticker.js`;
  `docs/logo/README.md`), `change-size.js` ("+125 −10", "4 files"),
  `disclosure.js` (a button that shows or hides an action behind a fold), and `diagrams.js`,
  which draws each diagram of a region that was built.

To add a screen or a region: give the view the data it needs (a field of `PageView` or of
the type of its screen, in `src/view.rs`, built from the round's snapshot), run
`make explore-types`, write the module that builds the region from that data, add its
region to `Page` in `page.js`, and list the module in `ASSETS`. An action the region
offers is a form with a `data-method`, an entry in `CALLS`, a variant of `Call` in
`src/rpc.rs`, and its check in `src/actions.rs`, which hands it to the owner as a
`PageCommand`. A new field of the view is one line on each side.

The client's types come from the Rust types of the socket's messages: ts-rs generates
`assets/client/types.ts` from them (`src/typescript.rs`), and each module names them in
its JSDoc (`/** @import { PageView } from "./types.ts" */`). The file is committed. A
Rust test fails once it no longer matches the Rust types (`make explore-types` writes it
again), and `make e2e-explore`, in `make check`, first runs `tsc` over the client
(`tests/explore-page/tsconfig.client.json`, with the `typescript` package of that
project), which fails on a field or a variant that one side no longer has. `tsc` emits
nothing: the browser loads the modules as they are.

The socket is tested in Rust (`src/socket.tests.rs`: admission, the view at each change,
a repeat of each action), and what the reviewer does on the page with the e2e tests
below; the client has no unit tests.

### Serve the page alone

```sh
nix develop --command make explore-page
```

This runs the standalone server
([`crates/review-explore-page-server`](../crates/review-explore-page-server)),
with no pane, no agent and no Herdr. It prints the address of a page that shows
a fixed question: `http://127.0.0.1:8790/?token=dev`. Open it in a browser.

The server reads the shell, the client's modules and the stylesheets from disk on
every request, and an open page loads itself again when one of them changes (its
file watcher answers `/dev/changes`; nothing polls the files), so a client or style
edit shows at once. A Rust edit needs a rebuild: stop the server and run the command
again; the open page reconnects by itself once the server is back. The token stays
`dev`, so the page opens again without a new address.

The server's own options: `--port N` (`0` picks a free port), `--token T`
(random when omitted), `--data short|rich`, what the agent posts, `--dev DIR`, the
page crate's directory to read the files from, and `--fixed-clock`, which stamps every start
and turn with the same time, and the chat's messages one second apart from that time (the
gallery uses it, and holds its pages' clock 42 seconds later, so that a time since reads
"0:42" in every run). Without `--dev`, it serves the files built
into the binary. The `short` data set, the default, is the one the e2e tests check; the
`rich` one is as long as a real round (a design in four full parts, questions with several
paragraphs of Context, tables, diagrams, three citations of three files of a change of 43
files, most of them under one deep directory, a one-way question, a diagram that does not parse, a conclusion with a ten-line list and a
quiz of three items), for the gallery below; `make explore-page
EXPLORE_PAGE_ARGS='--data rich'` serves it. Both are in
[`round_data.rs`](../crates/review-explore-page-server/src/round_data.rs). A test of the
rich round, such as the meter's with the 43 files, is registered with `richTest` from
`tests/explore-page/tests/session.ts` instead of `test`: its session's agent posts the rich
data set whatever the server's `--data`.

### Screenshot gallery

```sh
nix develop --command make explore-gallery
```

This takes a full-page screenshot of every state of the page, at 1280 and 390
pixels wide, in the light and the dark theme, and writes them with a contact
sheet, `index.html`, which shows each state's screenshots side by side. It starts
the standalone server with the rich data set and its fixed clock and, for each screenshot,
moves a fresh session of it to the state, in a page loaded at that width and theme, in the
UTC time zone and with its clock held still 42 seconds after the server's, through the
e2e fixture's helpers (`openSession` in `tests/explore-page/tests/session.ts`) and
exact actions on the page. It waits until the client has drawn the round, with its fonts
and diagrams, and has the reply to the last action it sent, then makes the window as tall
as the page, so that the
sticky column of the reviewer's actions shows whole rather than scrolling inside
itself, and shoots with the dev shell's headless Chromium. It calls no model, needs
no network once the npm packages are installed, and is not part of `make check`. A
state that fails to reach its page stops the run with its name.

Each image is named `<state>-<width>-<theme>.png`, so two runs compare file by file.
The variables, all optional:

- `GALLERY_DIR`: the folder to write, new or empty. By default a new folder under the
  system's temporary directory, which the run prints.
- `GALLERY_COMPARE`: an earlier gallery's folder. The contact sheet then shows before
  and after for each image that differs, marks the new ones, lists the ones that are
  gone, and can hide the images that did not change; the run prints the counts.
- `GALLERY_WIDTHS`: other widths, separated by spaces (`GALLERY_WIDTHS='1600 1280 900 390'`).
- `GALLERY_STATES`: only these states, by name, separated by spaces.

Two runs on the same code give the same files, on any machine: the fixed clocks and the time
zone make every time the page shows the same; an image differs only where the
browser draws differently. The comparison is byte for byte, so such a difference
also counts as a change. The states are listed once, in
[`tests/explore-page/gallery/states.ts`](../tests/explore-page/gallery/states.ts),
in the order of the contact sheet. To add a state of the page, add one entry there:
a name, which never changes once given, since it names the files; a line that says
what the state shows; and `reach`, which moves a fresh session, whose agent works on
its first question, to the state with the fixture's helpers, and leaves the page
showing it; the helpers of that file cover the usual paths (`after` for a move of the
round, `question`, `conclusion`). A state whose layout depends on a wide window names the
extra widths it is shot at in `extraWidths` (the `working` state is also shot at 2000
pixels). Before an action the round no longer offers, hold the page (`refused` in that file) so that it does not follow the round first. A state
that needs a new move of the round needs a control route of the server first, as
for an e2e test.

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
keep the judgement under a comment that starts with `// judgement:` and gives
the reason. `make e2e-explore` fails on a judgement without one, in any script
under `tests/explore-page`.

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
answers it in the pane, `explore.answerAfterFirstPick()` answers it with the recommended choice
after a first pick of another, as a blind question on the page would, `explore.cancelAnswerInPane()` cancels that answer (the question
then shows its recommendation at once),
`explore.failDelivery()` fails the prompt the session sends (the conclusion's
implementation request while it sends one, else the agent's next turn),
`explore.agentDoesNotStart()` stands for an agent that does not start on that prompt, and
`explore.interrupt()`, `explore.conclude()` and `explore.reset()` move the
round to the other stages. An answer sent from the page puts the agent to work,
and `explore.answers()` returns what the page sent; `explore.diagramErrors()`
returns the diagram errors the page reported, as the review tool saves them.
The round opens on the design of the change, which carries a sequence diagram and a table
that fit a desktop window but not a phone's; the design's diagram is the page's first
(`Diagram 1`), and `explore.openDesign()` opens the design screen at a later stage, at the
address the design map of the rail's "Design ▾" links to (`#design`). The second
fixed question carries a diagram that draws and one that does not parse. From the second
question on, and with the conclusion, the agent's fixed response to the previous answer
shows above it. The session's review has a fixed name, which the start screen shows. A start sent from the page shows the round starting,
`explore.sendKickoff()` puts the agent to work on it (or stands for a round
started in the pane), `explore.failStart()` fails the start, and
`explore.starts()` returns the starts the page sent. `explore.reviewEverything()` leaves
nothing to review, so the start screen offers no start and a start fails for that reason,
and `explore.unreviewLine()` lets a round start again. The change's review marks follow the
round: Jev marks a line of it once a round runs, each answer marks what its question said it
would, and `explore.markByHand()` marks more lines by hand, which the meter shows at once. An Implement from the page
shows the request as being sent until `explore.deliverImplementation()`;
`explore.implementInPane()` sends the conclusion's request from the pane, and
`explore.implementations()` returns the lists the page sent. `explore.actions()`
returns, by name, the other actions the page sent (Stop waiting, Retry, Cancel
answer, Reset, a cancel of an implementation request, the chat's Retry), and
`explore.holdPage()` stops the page from following the round until it sends an action,
which is then refused, for a test of an action refused on a stale page.
In the chat, `explore.messages()` returns the messages the review threads saved, with the
place in the round each was written at and its quote, `explore.agentReplies()` plays the
agent's fixed reply to the latest one, and `explore.messagesNotDelivered()` stands for a wakeup
that did not reach the agent.
`explore.restartReviewer()` closes the page's socket and refuses a new one until
`explore.reviewerBack()`, as a reviewer that restarts. `explore.reopenBeforeSending()` and
`explore.reopenWhileSending()` stand for a reopen of the review while the session sends a
prompt (the request is then saved but not sent, or its delivery unknown; the turn stopped,
or its delivery unknown), `explore.becomeEarlierRound()` makes the round an earlier one,
and `explore.failStorage()` stands for a storage failure. `explore.concludeWithQuiz()`
concludes with a quiz of two items, and `explore.quiz()` returns what the reviewer
answered of it, as the review tool saves it. The server's control
routes are listed in
[`control.rs`](../crates/review-explore-page-server/src/control.rs); a
`question` step takes an optional JSON `Question` body for a question of the
test's own. The server's log of each
target is in
`tests/explore-page/.e2e/logs/`; a failed run prints its end. The run also
fails when the browser reports that the page broke its content security policy,
which allows scripts, styles, workers and the manifest only from the page itself, inline
styles for Mermaid's diagrams, images from the page and `data:` URLs (the tab's icon),
requests to the page and its socket only, and no form post. A
failing test
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

After a run of every test passes, with no `E2E_ARGS`, `make e2e-explore`
fails when the cache holds recordings the run did not look up: those of a test
that was renamed, changed or removed. Delete the files it lists and commit the
removal. `tests/explore-page/cache-lookups.ts` notes the lookups of such a
run.

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
`agent.prompt`, which waits until Herdr sees the agent working or blocked. An agent
working already counts at once; one that shows neither within Herdr's 5 seconds gets
`agent_prompt_stalled`, saved as `NotStarted`, which Retry may resend with the same
request. A courier thread sends the prompts in order, so the worker keeps serving
while Herdr waits. Delivery does not parse terminal output or wait for an idle agent,
unfocused pane, or empty composer. Review access binds to the native session when
available, or the foreground process group otherwise. Cancellation removes unsent
requests and a replaced conversation rejects them.
Delivery failures retain the literal input for Retry. The kickoff prompt carries the scope, the change description (jj only; Git has none) and the first turn identity; the agent submits
its first question directly, after the design explanation in `design`, which only the first turn
may carry and its question must carry. It opens with a thesis for the change, and each of its
four parts opens with its own; code that shows the design reads them through `Design::thesis` and
`Design::parts`, which give a thesis in every case: for a round saved before theses existed, the
overview's first paragraph stands in for the change's thesis, and each part's first paragraph
(the overview's next one) for its own, on one line and left out of the part's text. Only a
paragraph of its own counts, not one inside a list or a quote; a part without one shows its
first line of text and keeps all of it, and an overview of one paragraph shares the change's
thesis. A message to the page carries those, never `Design`
itself, which saves such a round's parts as the plain strings they were. The pane shows it before the first question, and the page
shows it open above the first question and folded in every later stage of the round. The tools advertise complete schemas derived from the
shared submission types; the kickoff carries behavior instructions without schema
examples. A round's writing style (`WritingStyle` in
[`crates/review-explore-round-settings`](../crates/review-explore-round-settings)) comes from
the reviewer's settings for the next round (`ExploreRoundSettings`, saved by `ReviewStore` in
`settings.json`, changed in the pane with `W` through `SettingsAction::SaveExploreWritingStyle`) when
the round starts; `Exploration` and every `TurnRequest` keep it, so it survives a restore. In
Simplified Technical English the kickoff ends with the style's rules (`writing.md`) and every
later turn with a short reminder (`writing_wakeup.md`), since a long round's agent may have
compacted the kickoff; the plain style adds nothing. The runner formats later wakeups as the later-turn rules (`wakeup.md`: interpretation,
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

### Run-ahead

Run-ahead forks the pane agent's session while a question waits (setting `RunAhead` in
[`crates/review-explore-round-settings`](../crates/review-explore-round-settings), pane key
`z`, saved under `explore_round` in `settings.json`, read after each input of the session:
turned off, it discards the forks that run). A bare answer continues as the fork of its
choice, when that fork submitted its turn (below); any other answer runs the plain chain. The
pieces:

- [`crates/agent-fork`](../crates/agent-fork): a fork process that does not outlive the
  reviewer. `Launcher` starts every fork from one thread that lives as long as it, through
  `reviewer-control fork-exec <reviewer pid> <program> <arguments>`, which arms
  `PR_SET_PDEATHSIG(SIGTERM)` (the signal fires when that thread ends), checks that the reviewer
  still runs, then runs the program in its own place. A fork stays in the reviewer's process
  group, so the hang-up of the pane's terminal reaches it too: never give it a process group or
  a session of its own. `RunningFork::terminate` sends SIGTERM, then SIGKILL three seconds later.
  `ProcessStamp` (process ID and start time) names a process across a restart;
  `stop_recorded` stops a fork a stopped reviewer left, when its command line still holds its
  session ID. Only Linux has the parent-death signal: elsewhere, macOS included, `fork-exec`
  refuses and `ClaudeForks` takes no fork, so every answer runs the plain chain, with the
  reason.
- [`crates/review-run-ahead`](../crates/review-run-ahead): the records saved beside each round
  (`RoundForks`, `ForkRecord`: question, choice, the answer ID the fork was told, session ID
  chosen before the start, the session it was taken from, transcript directory, the reviewer's
  and the fork's `ProcessStamp`, the kept turn until the fork is discarded, its end and tokens,
  why it was discarded, whether it is cleaned up; `RoundForks` also counts the forks that
  failed in a row), and `ForkHost`, the interface to the agent whose session is forked.
- [`crates/claude-fork`](../crates/claude-fork): `ClaudeForks`, the `ForkHost` for Claude Code.
  It reads the pane agent's process from `/proc` (program, arguments without those that pick a
  session, an interactive mode or a prompt, working directory, environment without `HERDR_*`;
  `arguments.rs` lists every option of `claude` with its values, and an option it does not know,
  or a `--settings` of the agent's own, means no fork: a fork starts with the agent's exact flags
  or not at all),
  its transcript (`<config>/projects/*/<session>.jsonl`: the last `user` or `assistant` entry
  tells that the session moved, the last assistant entry gives the model), and starts
  `claude <pane arguments> -p --resume <session> --fork-session --session-id <new>
  --output-format stream-json --verbose --settings <hook> --allowedTools <the two submit tools>
  --model <model>`. The hook is `reviewer-control fork-guard`: no tool that writes, no MCP tool
  but the two submits, and only reading shell commands; a hook keeps the tool list, and so the
  prompt cache, as the agent's. A fork's tokens are the sum of its own assistant messages, by
  message ID. Herdr's `pane.agent_status_changed` subscription tells when the agent is idle
  (`HerdrClient::forward_agent_status_while` reports the current status once subscribed, then
  each change); nothing polls. Dropping `ClaudeForks` lets the discards under way finish.
- `review-explore-session/src/run_ahead/`: after every input, the session watches the question
  that waits (`run_ahead_reconcile`) and discards the forks of one that no longer does. Forks are
  taken once the agent is idle, again when it worked and its session moved; each gets the prompt
  `PreparedTurn` writes for its choice, with its own request, answer and access value, and the
  unreviewed lines as they will be after the answer (the question's marks applied to a copy of
  the review marks). A fork's submit is checked on a copy of the round and kept in
  `<round>.forks.json` beside the round; any call with a discarded fork's access is refused.
  Answer, Cancel answer, Reset, a new round, turning run-ahead off and closing the reviewer
  discard the forks; a reopened reviewer discards the forks a stopped reviewer left in the
  rounds of every review of the repository (`ReviewStore::rounds_with_forks`; forks of a
  reviewer that still runs stay its own). A question that forks of another running reviewer
  answer is not forked again: the record is checked under its lock before the forks start. The
  state watcher ignores `.forks.json`, as it ignores editor views.
- Fork failures. A fork that ends, or cannot start, without a kept turn before it is discarded
  failed (it crashed, was rate-limited, never submitted, or submitted only turns the tool
  refused): its choice is not prepared, and the log and its record say how it ended. After
  `FAILURES_TO_HALT` (3) failures in a row, `RoundForks::halted_at_ms` stops run-ahead for the
  round: its forks are discarded (`DiscardReason::TooManyFailures`), the pane shows a notice,
  and every later answer of the round runs the plain chain (`PlainReason::NoForks` with the
  reason). A kept turn starts the count again.
- `review-explore-session/src/run_ahead/answer.rs` and `switch.rs`: what an answer to a watched
  question does. `deliver_turn` saves the answer and applies its marks as always, then asks
  `run_ahead_answer`, which records the path and its reason (`TurnPath`, `PlainReason`) in
  `RoundForks::answers` and discards the forks the answer does not continue as.
  - The checks. The answer continues as the fork of its choice only when that turn is exactly
    the one the pane's agent would take for it: a choice (not None of the above) with no comment
    (white space is none, in the prompt too), a fork that submitted, no message of the reviewer
    in the round's conversation, under the question or since it was asked, that the forks do not
    hold (one posted once they were taken, or one the agent has not answered: a talk the agent
    answered moved its session, and the forks taken again after it hold it), the agent idle with
    an empty input box (`ForkHost::input_is_empty`), on the session the forks were taken from, whose
    last entry did not move, and a prompt and unreviewed diffs that are the fork's but for their
    identities (request, answer, access, diffs directory). Anything else runs the plain chain.
  - The switch. The turn's dispatch begins (the page shows the agent working) and
    `ForkHost::switch` runs on a thread of its own. `ClaudeForks` (`claude-fork/src/switch.rs`)
    waits until the fork's stream shows the answer to its submit (at most 10 s, or until the
    fork ends), stops the fork and keeps its transcript, checks again that the agent is idle
    with an empty box, submits `/resume <session>` through `agent.prompt` without a wait, then
    asks Herdr every 100 ms, for at most 20 s, until it reports the agent on that session, idle,
    with an empty box: Herdr sends no event when a session changes.
  - After the switch. The session pins the agent on that session (`PinnedAgent::new`) and
    records it as `last_agent_session`, commits the fork's turn under the answer's identities
    through the agent's own commit path, then records the dispatch as delivered. The agent's
    access and diffs are the fork's, as after a prompt. The question shows only once the switch
    is done; a turn saved meanwhile (after a Cancel answer, say) waits, and its prompt goes out
    once the switch ends, to the session the agent then runs.
  - A failure. A switch that fails, or a fork's turn that cannot be saved, marks the turn failed
    with the reason, and Retry runs the plain chain. A fork the agent was told to resume keeps its
    transcript (`Continuation::Failed { typed: true }`); a fork whose session may be the agent's
    is never discarded nor cleaned up.
  - Settling (`settle.rs`). The agent's session must match the round: a session that holds a
    turn the round does not have would take the answer a second time. When the agent may run a
    fork's session whose turn the round did not take (the switch failed once `/resume` was
    typed, which Claude Code may still carry out late; the reviewer stopped waiting, cancelled
    the answer or reset during the switch; the fork's turn was not saved; a reviewer stopped
    during the switch, which leaves the record `Switching`), `ForkHost::resume` has the agent
    resume the session the fork was taken from (`ForkRecord::from`), then the fork is discarded
    (`Continuation::Undone`). Claude Code takes what is typed in its pane in order, so once that
    `/resume` is typed and Herdr reports the agent there, an earlier late resume cannot move it.
    This rests on Claude Code taking what is typed while it loads a session after it, in order;
    it is not checked.
    When the round did take the fork's turn (a reviewer stopped between saving it and recording
    the switch), the agent resumes the fork's session instead. It settles at once after the
    switch, before a turn's prompt goes out (`run_ahead_hold`) and before forks are taken. A
    turn saved meanwhile waits; when the agent cannot be settled, the turn waits for Retry,
    which tries again, and no prompt reaches it. A resume Herdr did not confirm is not typed
    again without the reviewer asking.
  - No prompt reaches the agent while it switches or settles: the session holds
    `PromptSender::hold`, which keeps the thread service's courier, and so the round
    conversation's wakeups and the prompts to every other agent, from sending until the switch
    or the settling ends. The switch or the settling starts on a thread of its own once a prompt
    the courier was sending is sent (`PromptHold::drained`), so the session never waits.
  - Cancel answer after a prepared turn. The round forgets the answer and the fork's turn as
    for any turn. The agent knows that answer by the ID its fork was told, so the next prompts
    name it so in their `Cancelled answer:` lines (`RoundForks::answer_as_told`).
  - What the reviewer sees. The path of each answer's turn (`TurnPath`, `PlainReason`, in the
    tiny crate [`crates/review-turn-path`](../crates/review-turn-path), which the records, the
    pane's events and the page share) reaches the page as `TurnResponse::path` and the pane as
    `ExploreTurnPath` (and `ExploreRestored::turn_paths`, from `RoundForks::turn_paths`): a
    prepared turn once the agent runs its fork's session, a plain chain as soon as the answer is
    recorded. Both show `TurnPath::line`, the one place of its wording, with the turn: the dim
    line under the question or conclusion in the pane, the previous turn's foot on the page.
    "Prepared while you were thinking", or "Not prepared: " and the reason in plain words. A
    check that could not be made says nothing there (`Unchecked` is in the log), nor does a
    question run-ahead did not watch (run-ahead off). A failed switch (`SwitchFailed`) and one
    the reviewer stopped waiting for (`Withdrawn`) leave the turn waiting for Retry, with the
    failure's reason; the turn Retry brings says why it was not prepared. The standalone server's `question-not-prepared`
    step and the gallery's `question-2-not-prepared` state show a plain chain's line.

Tests: `review-explore-session` checks the session with a fake `ForkHost` (prompts, access,
discards, records, each reason of the plain chain and the path the pane and the page are given,
forks taken again after a talk in the chat or in the pane and used by the next bare answer, a
switch, a failed one, a turn held during a switch, each failure of a fork and the stop after too
many, Stop waiting, Retry, Cancel answer and Reset during a switch, from the pane and the page,
settling after a reopen, another reviewer's forks); `reviewer` checks real forks on an isolated
Herdr with a forkable Claude Code stand-in (`runtime/run_ahead.tests.rs`; while
`prompt.unreported-resumes` exists in the server's directory, the stand-in does not report its
resumes to Herdr): the stand-in in the pane takes `/resume <session>` as Claude Code does, reporting the session to
Herdr as Claude Code's session hook does (with a newer `--seq` and `--session-start-source
resume`; without them Herdr kept reporting the first session), and a fork stand-in prints its submit's answer
once the test submitted for it; and the parent-death signal (`tests/fork_lifetime.rs`). Two tests
run real Claude Code turns on your subscription, at `claude-sonnet-5-5`, and are ignored by
default:

```sh
cargo build -p reviewer --bin reviewer-control -p review-mcp-config --bin reviewer-mcp
cargo nextest run -p reviewer a_real_claude_code --run-ignored only
```

The second of them, `a_real_claude_code_agent_continues_as_the_fork_of_a_bare_answer`, answers
the first question bare with a choice whose fork submitted, checks that the pane resumed the
fork's session and that the fork's turn is the round's, then sends the next answer and has the
real agent take it in the fork's session (`claude -p --resume <fork>`).

They write the agent's and the forks' transcripts in your Claude Code configuration and delete
them at the end.
