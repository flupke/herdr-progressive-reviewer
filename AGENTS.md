# Standards

- Name things with the terms of `CONTEXT.md`, in code, docs and UI text, and use
  none of the synonyms it lists to avoid.
- Keep crates small, don't hesitate to create tiny ones or split a big one.
- Reuse existing types between workspace crates. Do not create a local copy of
  a type only to adapt it for another crate.
- Reuse shared UI components for rendering, editing, focus and keyboard behavior.
  Do not duplicate their internals or intercept component-owned keys in
  feature-specific wrappers. Extend the shared component when behavior is missing,
  then reuse it across the affected views.
- Design types first, then implement their methods.
- Use `#[must_use]` only when ignoring a return value is likely to cause a bug.
  Do not add it to routine getters or to functions that return types that
  already have this attribute.
- Always use the smallest visibility level that permits the required use. Do
  not make an item `pub` when private or `pub(crate)` visibility is sufficient.
- Make every unit test go red on a plausible bug in the code it calls; a test
  that cannot is tautological. Before writing or code-reviewing a unit test,
  read `.agents/wiki/unit-tests.md`: the kinds of tautological test, and what
  to assert in a prompt test.
- Refresh filesystem-driven views through filesystem events (inotify on Linux),
  not periodic polling. Reuse the repository watcher and event pipeline so idle
  views do not spend CPU checking for file changes.
- With the current `eyre` version, use `Err(eyre::eyre!(...))` in
  expression-position match arms, or put `eyre::bail!(...);` in a statement
  block, to avoid the trailing-semicolon macro warning.
- Before writing an Explore page e2e test, read
  `.agents/wiki/explore-page-e2e.md` and use the `e2e` skill.

# Wiki

The wiki is a playbook for working in this repository, one topic per page,
listed in `.agents/wiki/index.md`. Read the page for a tool (the checks,
`make vision`, the e2e tests, the Jev evaluations) before you run or debug it.
The workflow says when to read the pages of an area (step 1) and when to update
them (step 2).

A line earns its place when an agent working here needs it and cannot get it
cheaply from the code, the tests or a `--help`:

- how to run a tool, and its gotchas;
- the conclusions of expensive runs, such as evaluations and studies, and the
  commands that run them again;
- how an outside tool behaves, such as Herdr, jj, e2e, Claude Code, Codex and
  the language servers;
- the reason for a choice, or a guarantee that spans several files, which no
  single file can state.

What a module does, its files, its classes and markup, and the behaviour a test
checks stay in the code and the tests.

The rules for a page:

- Give a new topic its own page, and add its line to the index.
- A page says how things work now. The account of a past run goes in the change
  description, and its result files under `target/`: the page keeps only its
  conclusions.
- `docs/` is for people: design handoffs and the README's assets. Knowledge for
  agents goes in the wiki, and decisions in `.agents/adr/`.

# Decisions

`.agents/adr/` holds the decision records (ADRs), one decision per file. A
record keeps a decision from being undone by accident.

- When a change would contradict a record, tell the user which record, and why
  its reasons may no longer hold. Change the design once the user agrees.
- Record a decision when all three hold: it is hard to reverse, it would
  surprise a reader who lacks the context, and a real alternative was weighed.
  Say the situation, the choice, and the alternative given up with the reason.
- A record is frozen. When a decision changes, write a new record that names the
  one it supersedes, and leave the old text as it was.

# Change workflow

Follow these steps for every change: a feature, a fix, or a change to docs.

1. Create a fresh jj change before implementation. Use the previous change as
   the fixed point for this change. Read the wiki pages and the decision
   records of the area you will change.
2. Implement the change, and check it as you go with the levels that fit it,
   cheapest first:
   1. `make lint`: clippy on every target, the Explore page client's types, and
      the complexity gate.
   2. `make test CRATES="crate-a crate-b"`: the unit tests of the crates the
      change touches.
   3. `make test`: every unit test of the workspace.
   4. The end-to-end tests of what the change touches: `make e2e-tui` for the
      pane, `make e2e-explore` for the Explore page.
   Choose the levels at your discretion: run the Explore page tests only for a
   change that reaches the page, which
   `make check-changed CHECK_CHANGED_ARGS=--dry-run` lists with the other checks
   a change reaches. Once the change is complete, run
   `make fmt` and bring the wiki up to date: edit each line the change made
   false, and add a line only when it earns its place (see Wiki). Record a
   decision that the change made, when the decision passes the test in
   Decisions.
3. For a change the user can see, explore the affected paths in the real UI
   with `make vision` (see `.agents/wiki/tui-vision.md`) and
   fix what it finds.
4. Invoke the `code-review` skill against the fixed point; the subagents it
   starts are authorized. Fix its findings, by your judgement for a judgement
   call, and review again until a round finds nothing that you fix, for at most
   two rounds. After the second round, fix what it found, and report any
   finding you leave unfixed, with the reason.
5. Once step 4 is over, run the gate, with the change as the working copy:
   `make check-changed`. It runs only the checks that the files changed since
   the change's parent reach, and says why; for a stack of changes, set
   `CHECK_BASE` to the revision below the first. Fix whatever fails, even outside the change, run
   `make fmt`, review those fixes with one round of step 4, and run the gate
   again until it passes.
6. Check the wiki against the change once more, since the fixes of steps 4 and
   5 count too. Then describe the change with `jj describe`: a plain imperative
   subject, then what changed for the user and why.
7. Run `make install` once the gate passes, unless the user deferred
   installing; say so when you skip it. It builds and installs only; it does
   not run the checks.
8. Keep later user-feedback fixes in the same change, and run the gate again
   once each set of them is done. Create another change only when the user
   asks for something new.

Run the make targets through `nix develop --command` so the pinned Rust
toolchain, `cccc`, and `cargo-nextest` are available, and `make install` through
it too so the installed build uses the same toolchain. Keep the warning gate of
the checks (`-Dwarnings`) enabled; fix warnings in the current change instead
of overriding `-Dwarnings`.

Build what the user asks for yourself, in the current workspace, so the
user sees each change live. Supervise subagents in separate jj workspaces only
for a large feature planned as a graph of tickets, or for many independent
changes at once. When you work in a jj workspace, supervise agents there, or run
gates in parallel, read `.agents/wiki/parallel-workspaces.md`: how to enter
the dev shell, keep work visible, and run several gates at once.

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

# Skill settings

## Agent skills

### Issue tracker

Issues live in GitHub Issues for flupke/herdr-progressive-reviewer (via `gh`). See `.agents/settings/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `.agents/settings/triage-labels.md`.

### Domain docs

Single-context: root `CONTEXT.md` plus `.agents/adr/`. See `.agents/settings/domain.md`.
