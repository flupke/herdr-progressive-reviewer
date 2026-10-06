# Explore page: architecture

Why the Explore page is built as it is: the socket and the checks on each action, the actions for each state of a round, the agent's Markdown and diagrams, what the session publishes, the page's address, and the chat.

The Explore page is a browser page that shows an Explore round. Its routes,
socket and client are in
[`crates/review-explore-page`](../../crates/review-explore-page): axum serves a shell
page that loads a small JavaScript client (`assets/client`, see
[The page's client](explore-page-client.md)), with no framework, no bundler and no
package manager. Each open page holds one WebSocket at `/ws`: the tool sends the round
as typed data (`PageView` in `src/view.rs`), whole, when the socket opens and at each
change, and the client draws every screen from it and changes in place what changed,
so the reader's scroll, focus, selection and typed text stay. Before it hands an action
to the owner, the page checks that the round still offers it, by the identity of what the
action acts on; the owner checks again against its own round. A repeat of an action that
went through is answered as applied already and changes nothing; a repeat of a Start, a
Stop waiting, a Retry or a resend names a start, a turn or an attempt that is no longer the
current one, so it cannot act twice (`CommandRefusal::AlreadyApplied`, `src/actions.rs`).

On a question whose Door is not two-way, the page hides the recommendation until the
reviewer's first pick (`src/blind.rs`), unless the reviewer answered the question
before and cancelled the answer: the view the page holds before that pick carries
neither the agent's reason nor any mark of the recommended choice, and the choices come
in a mixed order. The first pick is a request of its own (`PageCommand::Pick`), which the
page keeps (`FirstPicks`), and the answer carries it to the owner as
`AnswerInput::first_pick`.

The page offers every action of the pane's Explore tab for the round's state,
through `PageCommand`, and the session carries each one out by the path of the
pane's command, so that it saves the same result and sends the same prompt
(`crates/review-explore-session/src/page_actions.rs`); the pane's own behaviour does
not change. On the network, Reset ends the round's token, and the page that sent it
receives the start screen's next token in the reply (`Rounds::after_reset`, ADR 0003), and
opens again with it.

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
[`crates/markdown-html`](../../crates/markdown-html), which shows raw HTML as text
and keeps a fenced block's language as the class `language-<name>` of its
`<code>`. Callouts (`> [!TIP]`) and table-cell status marks (`[!good]`) are
defined once in [`crates/markdown-marks`](../../crates/markdown-marks): the page,
the pane's renderer and the kickoff prompt all read them from there.

The page follows the reviewer's design handoff in
[`docs/design/explore-page/`](../../docs/design/explore-page/README.md): its `README.md` is the
specification, `screenshots/` the reference captures, and `design-review.md` the detailed
findings.

A fenced `mermaid` block is a diagram, which `assets/client/diagrams.js` draws in the
browser with Mermaid, every diagram of the page the same way (design review, finding 30):
whole, as the project owner asked, shrunk to fit and never enlarged. Its nodes classed
`new` or `changed` are drawn in green and amber; the kickoff's diagram rules
(`crates/review-explore-runner/src/diagrams.md`) name that vocabulary. Mermaid is
vendored, pinned and gzipped at build time in
[`crates/mermaid-js`](../../crates/mermaid-js) (its `vendor/README.md` says how to
move to another version); the page serves it itself, loads it only when it
shows a diagram, and lets the browser keep it, since its address names its
version. The tool cannot check a diagram when the agent submits it: when Mermaid
cannot parse one, the page shows its source and sends the error over the socket
(`diagram-failed`), and the session (`ExploreSession`) saves it with the question
(`Exploration::diagram_errors`).

In the reviewer, the session (`ExploreSession`) publishes the stage of its round after
each input it handles, with the lines of the change that each citation of a question names
(`Comparison::tracked_cited_lines`, for files the change touches or the repository tracks
only, since the page may be open from the network), and
[`crates/review-explore-page-host`](../../crates/review-explore-page-host) serves
the page of that round on a loopback port behind a token. Whether a round can start follows
one rule, `review_explore::StartBlock`: not once every changed line is marked as reviewed.
The session tells the pane (`ExploreStartBlock`) and the page (`RoundPublisher::block_starts`),
and it reads the marks again as it captures the change and before it sends a kickoff, so a
start that finds nothing left to review fails without one. The page's
commands join the session's inputs, in the same order as the pane's. A start from the page
returns the kickoff to the reviewer's worker (`ExploreSession::start_from_page`), which lets
Jev mark first as for a kickoff from the pane. Once the conclusion has a request that this
process sends or that the agent received, the session refuses another, from the page or from
the pane, so a repeated or stale Implement cannot start a second implementation; a
request an earlier process left paused or unknown stays the pane's to resolve.

The reviewer records the page's address, readable only by the user, under
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
it with the pane's thread commands (`ThreadSender`); the reviewer's side is in
`crates/reviewer/src/runtime/page_threads.rs`. A message carries the identity the page chose
for it, so that a repeat posts it once; it wakes the agent with the threads' usual prompt,
which names the round and the question and says that it is no answer.
