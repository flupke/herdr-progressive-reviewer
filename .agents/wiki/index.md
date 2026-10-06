# Wiki index

A playbook for working in this repository, one topic per page. `AGENTS.md` holds the rules that always apply, says what earns a line here, and when to read a page and keep it current.

Elsewhere: the glossary is [CONTEXT.md](../../CONTEXT.md); decisions are in [.agents/adr/](../adr/); the Explore page's design handoff is in [docs/design/explore-page/](../../docs/design/explore-page/README.md); procedures are skills in [.agents/skills/](../skills/), and the settings those skills read are in [.agents/settings/](../settings/).

## Working in the repository

- [Checks](checks.md): gotchas of the check commands, the Herdr release the tests run, the opt-in tests, mutation runs.
- [Unit tests](unit-tests.md): tautological tests, which code review removes, what makes a test go red on a bug, how a test waits.
- [Parallel workspaces](parallel-workspaces.md): running a gate or an agent in a second jj workspace.
- [Terminal UI exploration](tui-vision.md): driving the real pane with `make vision`, the gotchas of its commands and stand-ins, turning findings into regression tests.
- [Explore page: e2e tests](explore-page-e2e.md): writing and running the page's e2e tests: checks by locator, the replay cache and the model, writing a test with an agent.
- [Explore page: standalone server and screenshot gallery](explore-page-standalone.md): `make explore-page` and `make explore-gallery`.

## Reviewing files and threads

- [Jev](jev.md): enabling it, where it runs and what it sends, the evidence behind its production settings.
- [Jev evaluations](jev-evals.md): the splitting comparison and the prompt experiment, opt-in.
- [Jev history study](jev-history-study.md): the corpus, the method and the commands of the history study and the compact query comparison.
- [Review threads](threads.md): which agent gets a comment, Herdr's wakeup delivery, access values.
- [MCP bridge](mcp-bridge.md): registration with Codex and Claude Code and its gotchas, port conflicts.
- [Language servers](language-servers.md): direnv, project roots, limits in Explore.
- [Reviewer runtime](runtime.md): recording UI timings.

## Explore rounds

- [Explore round](explore-round.md): what a round guarantees the reviewer, and what its judgements and marks do not establish.
- [Explore agent contract](explore-agent-contract.md): why the prompts and tools are shaped as they are, and what a submission guarantees.
- [Explore recovery](explore-recovery.md): what is never saved, what a crash can lose, and the durable record of each delivery.
- [Run-ahead](run-ahead.md): what forks cost and need, the guarantees across crates, how Herdr and Claude Code behave, the real Claude Code tests.
- [Explore settings](explore-settings.md): where the settings are saved, and when another reviewer sees a change.
- [Explore statistics](explore-statistics.md): how to run `reviewer-control stats`, and the limits of its numbers.

## Explore page

- [Explore page: architecture](explore-page.md): the socket's checks, the actions for each state of a round, Markdown and diagrams, what the session publishes, the page's address, the chat.
- [Explore page: client](explore-page-client.md): how its modules are served, adding a screen, the generated types and their check.
- [Explore page: network and tunnel](explore-page-sharing.md): what the network listener and the tunnel expose, and the test stand-ins.
