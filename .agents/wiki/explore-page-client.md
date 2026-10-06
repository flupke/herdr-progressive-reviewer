# Explore page: client

The page's JavaScript client: its modules, its rendering rules, how to add a screen or a region, and where its types come from.

The client is plain JavaScript ES modules in
[`crates/review-explore-page/assets/client`](../../crates/review-explore-page/assets/client),
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
  pixels or a flick (the design handoff's README, "Swipe between screens"); a drag that starts in a frame that
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
again), and `make e2e-explore`, in `make check-with-e2e`, first runs `tsc` over the client
(`tests/explore-page/tsconfig.client.json`, with the `typescript` package of that
project), which fails on a field or a variant that one side no longer has. `tsc` emits
nothing: the browser loads the modules as they are.

The socket is tested in Rust (`src/socket.tests.rs`: admission, the view at each change,
a repeat of each action), and what the reviewer does on the page with the
[e2e tests](explore-page-e2e.md); the client has no unit tests.
