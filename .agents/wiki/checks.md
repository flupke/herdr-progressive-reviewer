# Checks

What each check command runs, the Herdr release the tests run, the opt-in tests, and mutation runs.

[AGENTS.md](../../AGENTS.md) lists the check levels (step 2) and says which gate a
change runs (step 5). What it leaves out:

- `make lint` also type-checks the Explore page's client with `tsc`, from the npm packages
  of `tests/explore-page`: the first run on a checkout installs them with `npm ci`, which
  needs the network.
- `make test` runs the doc tests of the workspace, then every unit test with nextest,
  which runs no doc test itself. With `CRATES`, it runs nextest alone.
- `make check` runs `make lint`, the static rule on judgements in the Explore page's
  tests, `make test` and `make e2e-tui`. It runs neither `make e2e-explore` nor
  `make vision`.
- `make check-with-e2e` runs `make check`, then `make e2e-explore`.

`tests/tui` is a separate Cargo workspace, with its own `Cargo.lock`, that uses the
workspace's crates: a search or a `cargo` command at the root misses it. Before removing
a public item, a derive or a dependency of a crate, search `tests/tui` for its uses
too.

No check fails on formatting. The test summary
names each test slower than 10 s, and `target/nextest/default/junit.xml` keeps every
test's time.

In addition to the build dependencies, the checks use Herdr, Codex, Claude Code,
Python 3, `cargo-nextest`, `cccc`, `jq` and Node, which `make lint` uses for the client's
type check. Herdr integration tests run private servers with isolated configuration,
state and agent paths. The [Explore page tests](explore-page-e2e.md) also use a headless
Chromium, which the Nix shell provides.

## The Herdr the tests run

The tests do not use the Herdr installed on your machine. The Nix shell fetches
a fixed Herdr release and points `TEST_HERDR_BIN_PATH` at it; the test servers
run that binary. `herdr` on the `PATH` stays the installed Herdr, which
`make install` uses. Outside the Nix shell, the tests fall back to `herdr` on
the `PATH`. Set `TEST_HERDR_BIN_PATH` inside the shell to try another binary:
`nix develop --command env TEST_HERDR_BIN_PATH=/path/to/herdr make check`.

Each test server also detects agents with the rules in
[`crates/review-test-support/agent-detection`](../../crates/review-test-support/agent-detection),
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
3. Run `nix develop --command make check-with-e2e`.

## Tests that call no model

Explore's domain, Git/jj comparison, durable inputs and isolated selected-agent MCP
tests use deterministic responses and call no model API. (The Explore page's e2e
tests in `make check-with-e2e` call a model only for a goal with no valid recording: see
[Agent steps and the model](explore-page-e2e.md#agent-steps-and-the-model).) Explore's optional UI
test uses real rust-analyzer to navigate working-copy sources and reject a
delayed result for another evidence window. A check of Explore with a real agent is manual,
in a disposable repository and a private Herdr server.

## Opt-in tests

None of these runs during `make check`.

Optional real-language-server tests run in temporary projects:

```sh
cargo test -p review-lsp --test language_servers -- --ignored
cargo test -p review-ui real_rust_lsp -- --ignored --nocapture
```

[Language servers](language-servers.md) lists the server commands.

The [`jev-evals` suite](jev-evals.md) compares Jev hunk-splitting strategies
against frozen line-level labels. Offline checks and paid live runs are separate;
neither runs during `make check`. The [history-based study](jev-history-study.md)
jointly compares prompts, metadata, exclusion rules and token windows on an
audited corpus from repository commits.

Two run-ahead tests run real Claude Code turns: see [Run-ahead](run-ahead.md).

## Mutation testing

Mutation testing has three run levels. For one file, use the
`cover-missed-mutations` skill (`.agents/skills/cover-missed-mutations`). For
changed code, `cargo mutants --workspace --test-workspace=true --test-tool=nextest
--in-diff <diff>` tests only the mutants that overlap the diff: it misses coverage
lost outside the diff, and a test-only change selects no mutants. `make mutants`
runs the whole workspace; run it on a schedule or before a high-risk release, and
read `mutants.out/` for its results. A mutation score of 100% is not the goal:
equivalent and low-value mutants stay missed.

`cargo mutants --list --diff --file <file>` shows the mutants of a file and their source
changes without running a test; `--re` and `--exclude-re` select mutants by function or
description. The mutation runs use nextest, which runs no doc test: the gate's doc tests
stay the only check of those.
