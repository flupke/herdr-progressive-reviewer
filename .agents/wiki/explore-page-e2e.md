# Explore page: e2e tests

What the page's e2e tests are for, how they are written and run, and the tools for writing one with an agent.

## What the suite keeps

The suite, in [`tests/explore-page`](../../tests/explore-page), is a set of journeys: the
reviewer's main paths through a round (start, design, questions and answers, the quiz,
the conclusion and Implement, following the pane, the chat, a reconnection, earlier
questions). They are chosen to run the client's own logic, which has no unit tests:
drafts kept across a reload or a stage change, reconnection, the request each control
sends, the chat's focus and unread count.

What a journey does not hold, and why:

- A rule the server enforces (who may load the page, the refusal of an action the round
  moved past, Retry, Stop waiting and the other recovery paths, the implementation
  request's states) is a Rust test, in memory: `crates/review-explore-page/src/socket.tests.rs`
  and `page.tests.rs`, and the tests of `crates/review-explore-session`.
- How the page draws a state (Markdown, diagrams, citations, the meter, the tab icon,
  layouts at each width, swipes) is not tested: look at it with the gallery, or act on
  it with `make explore-script` ([standalone server](explore-page-standalone.md)).

## Write a test

Write a test the way the [e2e documentation](https://e2e.tester.army/docs) and its
skill (`.agents/skills/e2e`) say, but with exact steps only: each step a locator and an
action (`screen.getByRole('button', 'Send answer').tap()`, `.check()`, `.fill()`), each
outcome a locator and a matcher. The config names no model, so an `agent.act`,
`agent.assert`, `agent.waitFor` or `agent.extract` fails: the suite is deterministic
and calls no model.

A test sets the round up through the `explore` fixture of
`tests/explore-page/tests/session.ts`, which plays the Explore agent and the reviewer's
pane; its `Session` interface documents each helper. A move of the round that no helper
makes needs a control route of the server first, in
[`control.rs`](../../crates/review-explore-page-server/src/control.rs).

Check the facts that make the outcome, not each element of the page: listing every
choice, count and sentence makes a test break on every markup change. A matcher waits
for the page, which follows the round a moment after the fixture moves it. Locators
address the page by roles and accessible names, so the markup must name what a test
looks for (a `<section>` labelled by its heading, a `<fieldset>` with a `<legend>`,
`role="status"`).

Gotchas of e2e 0.16:

- A role locator whose name holds an apostrophe fails with `ENGINE_FAILURE` (an invalid
  selector): match it with a pattern instead, `/^In the pane.s editor$/`.
- `count()` and the other reads do not wait: use a matcher for a value that settles.

## Run the tests

```sh
nix develop --command make e2e-explore
```

It runs every test at a desktop size, against its own server, in about 10 seconds.
`E2E_ARGS` go to `e2e run`, with paths relative to `tests/explore-page`:
`make e2e-explore E2E_ARGS=tests/round.e2e.ts` runs one file.
The first run in a checkout installs the npm packages from the committed
lockfile, which needs the network. The Nix shell provides Node and the headless
Chromium of nixpkgs (`E2E_CHROMIUM`), which the tests attach to instead of
Playwright's own download, and turns e2e's telemetry off
(`E2E_TELEMETRY_DISABLED`): run every `npx e2e` command inside it; `run.sh` refuses to
run outside it.

The server's log is in `tests/explore-page/.e2e/logs/`; a failed run prints its end.
The run also fails when the browser reports that the page broke its content security
policy, set in `crates/review-explore-page/src/page.rs`: only a browser can check
that. A failing test leaves the accessibility tree of the page and a Playwright trace
under `tests/explore-page/.e2e/artifacts/`.

## Write a test with an agent's help

The e2e skill, at `.agents/skills/e2e` (and `.claude/skills/e2e`), tells a
coding agent how to use e2e; its parts on agent steps, models and the replay cache do
not apply here. It is what `npx e2e init` writes for the pinned e2e version: when
`package.json` moves to another e2e version, run `npx e2e init --yes` in a scratch
directory and copy its `.agents/skills/e2e` over this one. Do not take the rest of what
`init` writes: its example test needs a model.

e2e's MCP server lets a coding agent open the page in a browser, act on it
(navigate, tap, type), read what it shows, and try a locator with `locate`
before writing it into a test. Register it once for Claude Code, from the repository
root:

```sh
claude mcp add e2e -- "$PWD/tests/explore-page/mcp.sh"
```

Install e2e once first with `nix develop --command make e2e-explore-deps`.

An agent whose session has no server registered can drive the same server from
the shell with `tests/explore-page/mcp-cli.mjs`, whose header shows how.
`/?token=e2e` opens the server's own round, which shows the fixed question. Each
run of `mcp-cli.mjs` starts a new page server.
