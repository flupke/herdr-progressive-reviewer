# Wiki index

What is known about how this repository works, one topic per page. `AGENTS.md` holds the rules that always apply, and says when to read a page and how to keep the wiki current.

Elsewhere: the glossary is [CONTEXT.md](../../CONTEXT.md); decisions are in [.agents/adr/](../adr/); the Explore page's design handoff is in [docs/design/explore-page/](../../docs/design/explore-page/README.md); procedures are skills in [.agents/skills/](../skills/), and the settings those skills read are in [.agents/settings/](../settings/).

## Working in the repository

- [Checks](checks.md): what each check command and gate runs, the Herdr release the tests run, opt-in tests, mutation runs.
- [Unit tests](unit-tests.md): tautological tests, which code review removes, and what makes a test go red on a bug.
- [Parallel workspaces](parallel-workspaces.md): running a gate or an agent in a second jj workspace.
- [Terminal UI exploration](tui-vision.md): `make vision`, the driver that operates the real pane, its commands and session files.
- [Explore page: e2e tests](explore-page-e2e.md): writing and running the page's e2e tests, the fixture, the replay cache and the model.
- [Explore page: standalone server and screenshot gallery](explore-page-standalone.md): `make explore-page` and `make explore-gallery`.

## Reviewing files and threads

- [Review marks in Files](review-marks.md): what marking a file or a hunk does, automatic marking, unmarking a change.
- [Jev](jev.md): when the classifier runs, what it is sent, the production settings and their evidence.
- [Jev evaluations](jev-evals.md): the splitting comparison and the prompt experiment, opt-in.
- [Jev history study](jev-history-study.md): the corpus, the method and the commands of the history study and the compact query comparison.
- [Review threads](threads.md): resolving and reading, which agent gets a comment, delivery, the comment tools, access values.
- [Conversation store](conversation-store.md): how threads and drafts are saved, and the migrations.
- [MCP bridge](mcp-bridge.md): the stdio bridge and the reviewer's server, registration, ports, what the tests verify.
- [Language servers](language-servers.md): which server starts for a file, in which environment and project root.
- [Reviewer runtime](runtime.md): the idle reviewer, filesystem refresh, UI timings.

## Explore rounds

- [Explore round: behaviour](explore-round.md): a round from Start to Reset as the reviewer experiences it.
- [Explore agent contract](explore-agent-contract.md): what the kickoff and each wakeup carry, and what the two Explore tools accept and refuse.
- [Explore protocol: internals](explore-protocol.md): how a submission is validated, committed and delivered, and what the pane renders.
- [Explore recovery](explore-recovery.md): the saved round, its bounds, and the durable record of each delivery.
- [Run-ahead](run-ahead.md): forks that prepare the next turn while a question waits.
- [Explore settings](explore-settings.md): the round settings and the page settings, and what run-ahead does for the reviewer.
- [Explore statistics](explore-statistics.md): what `reviewer-control stats` counts.

## Explore page

- [Explore page: behaviour](explore-page-behaviour.md): what the reviewer can do on the page and what the page guarantees.
- [Explore page: architecture](explore-page.md): the socket, the actions for each state of a round, Markdown and diagrams, publishing, the chat.
- [Explore page: components](explore-page-components.md): the catalogue of building blocks, with markup.
- [Explore page: client](explore-page-client.md): the JavaScript modules, the rendering rules, adding a screen.
- [Explore page: network and tunnel](explore-page-sharing.md): the network listener, the Cloudflare tunnel, and what each exposes.
