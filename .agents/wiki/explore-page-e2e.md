# Explore page: e2e tests

How the page's e2e tests are written and run: checks by locator, the replay cache and the model behind agent steps, and the tools for writing a test with an agent.

## Write and run a test

The page is tested only with [e2e](https://github.com/tester-army/e2e), in
[`tests/explore-page`](../../tests/explore-page). Write a test the way the
[e2e documentation](https://e2e.tester.army/docs) and its skill
(`.agents/skills/e2e`) say. A test sets the round up through the fixture below, then takes the steps the reviewer would take, each
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
the reason. `make check` and `make e2e-explore` fail on a judgement without one, in
any script under `tests/explore-page`.

Check the facts that make the outcome, not each element of the page: listing
every choice, count and sentence with locators makes a test break on every
markup change. A matcher waits for the page, which follows the round a moment
after the fixture moves it. Locators address the page by roles and accessible
names, so the markup must name what a test looks for (a `<section>` labelled by
its heading, a `<fieldset>` with a `<legend>`, `role="status"`).

```sh
nix develop --command make e2e-explore
```

It runs every test at a desktop and at a phone size, each against its own server.
`E2E_ARGS` go to `e2e run`, with paths relative to `tests/explore-page`:
`make e2e-explore E2E_ARGS=tests/round.e2e.ts` runs one file, and
`E2E_ARGS=--no-cache` runs without the replay cache.
The first run in a checkout installs the npm packages from the committed
lockfile, which needs the network. The Nix shell provides Node and the headless
Chromium of nixpkgs (`E2E_CHROMIUM`), which the tests attach to instead of
Playwright's own download, and turns e2e's telemetry off
(`E2E_TELEMETRY_DISABLED`).

Each test gets its own round on the server, through the `explore` fixture of
`tests/explore-page/tests/session.ts`, which plays the Explore agent and the
reviewer's pane; its `Session` interface documents each helper. A move of the round
that no helper makes needs a control route of the server first, in
[`control.rs`](../../crates/review-explore-page-server/src/control.rs). The server's log of each
target is in
`tests/explore-page/.e2e/logs/`; a failed run prints its end. The run also
fails when the browser reports that the page broke its content security policy,
set in `crates/review-explore-page/src/page.rs`. A
failing test
leaves the accessibility tree of the page and a Playwright trace under
`tests/explore-page/.e2e/artifacts/`.

## Agent steps and the model

`make e2e-explore` uses e2e's replay cache the way
[its documentation](https://e2e.tester.army/docs/cache) describes. A goal with a
valid recording under `tests/explore-page/.e2e/cache` replays without a model
call. A new goal, or one whose replay no longer matches the page, goes to the
model, and the cache is updated once the check after the goal passes. The tests
make no judgement, which e2e never replays, so a `make e2e-explore` whose goals all
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

## Write a test with an agent's help

The e2e skill, at `.agents/skills/e2e` (and `.claude/skills/e2e`), tells a
coding agent how to write and run e2e tests. It is what `npx e2e init` writes
for the pinned e2e version: when `package.json` moves to another e2e version,
run `npx e2e init --yes` in a scratch directory and copy its
`.agents/skills/e2e` over this one. Do not take the rest of what `init` writes:
its `.gitignore` lines would ignore the committed recordings, and its example
test fails in `make e2e-explore`.

e2e's MCP server lets a coding agent open the page in a browser, act on it
(navigate, tap, type), read what it shows, and try a locator with `locate`
before writing it into a test. Its sessions record nothing: `make e2e-explore` gains
no recording from them. Register it once for Claude Code, from the repository
root:

```sh
claude mcp add e2e -- "$PWD/tests/explore-page/mcp.sh"
```

Install e2e once first with `nix develop --command make e2e-explore-deps`.

An agent whose session has no server registered can drive the same server from
the shell with `tests/explore-page/mcp-cli.mjs`, whose header shows how.
`/?token=e2e` opens the server's own round, which shows the fixed question. Each
run of `mcp-cli.mjs` starts a new page server, and nothing it does is recorded.
