# Checks

Gotchas of the check commands, the Herdr release the tests run, the opt-in tests, and mutation runs.

[AGENTS.md](../../AGENTS.md) lists the check levels (step 2) and says which gate a
change runs (step 5); the `Makefile` says what each target runs. What neither says:

- nextest runs no doc test, so `make test` with `CRATES` runs none: only `make test`
  without it runs the doc tests, and `make check-changed` runs them only when it runs
  every check.
- `make check-changed` cannot tell a comment from code: a comment edited in a crate runs
  that crate's checks. `make lint` fails on a tracked file outside every crate that no
  table of `crates/check-changed/src/plan.rs` names, such as a stray file jj tracked.

`tests/tui` is a separate Cargo workspace, with its own `Cargo.lock`, that uses the
workspace's crates: a search or a `cargo` command at the root misses it. Before removing
a public item, a derive or a dependency of a crate, search `tests/tui` for its uses
too.

## The Herdr the tests run

The tests do not use the Herdr installed on your machine. The Nix shell fetches
a fixed Herdr release and points `TEST_HERDR_BIN_PATH` at it; the test servers
run that binary. `herdr` on the `PATH` stays the installed Herdr, which
`make install` uses. Outside the Nix shell, the tests fall back to `herdr` on
the `PATH`. Set `TEST_HERDR_BIN_PATH` inside the shell to try another binary:
`nix develop --command env TEST_HERDR_BIN_PATH=/path/to/herdr make check`.

Herdr's documentation leaves much of its behaviour out, such as what it does with a
pane's OSC 52 clipboard write. Read its source at the pinned release instead (`herdrVersion`
in `flake.nix`): `git clone --depth 1 --branch v<version>
https://github.com/herdrdev/herdr`. `src/pane.rs` handles what a pane's program writes,
and `src/selection.rs` the clipboard.

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

Explore's tests in `make check` use deterministic responses and call no model API; the
real Claude Code run-ahead tests are opt-in (Opt-in tests). (The Explore page's
e2e tests in `make check-with-e2e` call a model only for a goal with no valid recording:
see [Agent steps and the model](explore-page-e2e.md#agent-steps-and-the-model).) A check
of Explore with a real agent is manual, in a disposable repository and a private Herdr
server.

## Opt-in tests

None of these runs during `make check`.

Optional real-language-server tests run in temporary projects:

```sh
cargo test -p review-lsp --test language_servers -- --ignored
cargo test -p review-ui real_rust_lsp -- --ignored --nocapture
```

[Language servers](language-servers.md) lists the server commands.

The [`jev-evals` suite](jev-evals.md) compares Jev hunk-splitting strategies
against frozen line-level labels. Offline checks and paid live runs are separate. The [history-based study](jev-history-study.md)
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

The mutation runs use nextest, which runs no doc test: the gate's doc tests stay the only
check of those.
