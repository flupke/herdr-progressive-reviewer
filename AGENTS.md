# Standards

- Keep crates small, don't hesitate to create tiny ones or split a big one.
- Reuse existing types between workspace crates. Do not create a local copy of
  a type only to adapt it for another crate.
- Reuse shared UI components for rendering, editing, focus and keyboard behavior.
  Do not duplicate their internals or intercept component-owned keys in
  feature-specific wrappers. Extend the shared component when behavior is missing,
  then reuse it across the affected views.
- Avoid "functions soup", design types first, then implement their methods.
- Use `#[must_use]` only when ignoring a return value is likely to cause a bug.
  Do not add it to routine getters or to functions that return types that
  already have this attribute.
- Always use the smallest visibility level that permits the required use. Do
  not make an item `pub` when private or `pub(crate)` visibility is sufficient.
- Test what the code puts into a prompt (identity, answers, paths, ordering),
  never the wording of its instructions: a rewording must not break a test.
- Refresh filesystem-driven views through filesystem events (inotify on Linux),
  not periodic polling. Reuse the repository watcher and event pipeline so idle
  views do not spend CPU checking for file changes.
- Write Explore page e2e tests the way the e2e documentation
  (https://e2e.tester.army/docs) says, using the `e2e` skill at
  `.agents/skills/e2e`. Write a flow the reviewer performs as goals and the
  outcomes the reviewer sees, not as an enumeration of the page's elements.
  Check each outcome with `expect` on a locator, never with a judgement
  (`agent.assert`, `agent.waitFor`, `agent.extract`), unless no locator can
  reach the fact; then say why in a comment above it.
  Repository specifics: `docs/development.md#e2e-tests`.

# Small feature workflow

For each small feature:

1. Create a fresh jj change before implementation. Use the previous change as
   the fixed point for this feature.
2. Implement and validate the feature with `make check`.
3. For a change the user can see, explore the affected paths in the real UI
   with `make vision` (see `docs/development.md#llm-directed-exploration`) and
   fix what it finds.
4. Invoke the `code-review` skill against the fixed point; the subagents it
   starts are authorized. Fix its findings and repeat the review until it
   passes. Follow the skill's repair-loop limit and report any findings that
   remain when the limit is reached.
5. After the review passes, describe the change with `jj describe`: a plain
   imperative subject, then what changed for the user and why.
6. Run `make install` once the checks of step 2 and the review pass, unless
   the user deferred installing; say so when you skip it. It builds and
   installs only; it does not run the checks.
7. Keep later user-feedback fixes in the same change. Create another change
   only when the user requests the next feature.

Run `make check` through `nix develop --command` so the pinned Rust toolchain,
`cccc`, and `cargo-nextest` are available, and `make install` through it too so
the installed build uses the same toolchain. Keep the
`make check` warning gate enabled; fix warnings in the current change instead
of overriding `-Dwarnings`. With the current `eyre` version, use
`Err(eyre::eyre!(...))` in expression-position match arms, or put
`eyre::bail!(...);` in a statement block, to avoid the trailing-semicolon
macro warning.

# Sandbox E2E Tests

Herdr E2E tests start an isolated background server with private socket,
config, state, workspace, and agent paths. Never use, restart, or modify the
user's live Herdr server or panes during tests.

The Codex Linux sandbox blocks Unix-domain socket bind and connect operations.
Run tests in the normal sandbox first. If an E2E test fails with `EPERM` while
it creates or connects to its private socket, retry the same exact command
with sandbox escalation.

Do not enable project-wide network access. Do not replace the real Herdr
server with a mock only to avoid the sandbox restriction.

## Agent skills

### Issue tracker

Issues live in GitHub Issues for flupke/herdr-progressive-reviewer (via `gh`). See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: root `CONTEXT.md` plus `docs/adr/`. See `docs/agents/domain.md`.
