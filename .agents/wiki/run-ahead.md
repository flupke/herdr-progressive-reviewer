# Run-ahead

What run-ahead costs and needs, the guarantees that span its crates, how Claude Code and Herdr behave under it, and its contract check with real Claude Code.

Run-ahead forks the pane agent's session while a question waits (setting `RunAhead` in
[`crates/review-explore-round-settings`](../../crates/review-explore-round-settings), pane key
`z`). A bare answer continues as the fork of its choice once that fork submitted its turn, and
waits for a fork still at work; Stop waiting, or a fork that ends without its turn, gives the
answer to the agent in the pane. Any other answer runs the plain chain. The module headers of
`agent-fork`, `claude-fork`, `review-run-ahead` and `crates/review-explore-session/src/run_ahead.rs`
say how each part works.

## Cost and needs

- Run-ahead is experimental. A talk in the round conversation is its main waste: each message
  changes what the agent knows, and each set of forks re-reads the agent's whole prompt cache.
  So a message the reviewer posts in the round conversation discards the forks of the question
  that waits at once (the thread worker tells the session, `Input::RoundMessage`), and forks
  are taken again only once the agent replied to every message of the talk and the round
  conversation stayed quiet for a minute (`TALK_QUIET`, injected through
  `Collaborators::talk_quiet`).
- Each fork costs about one agent turn; it reads the agent's prompt cache.
- It needs Claude Code in the agent's pane, started with options run-ahead knows and
  without a `--settings` of its own, and with the hooks of the reviewer's Claude Code plugin
  (below): the switch waits for them. An agent runs with them when the session Herdr reports
  is the one the plugin's `SessionStart` hook recorded for its pane, from a process still in
  the pane. Without them (an agent started before the install, or with the plugin disabled),
  run-ahead takes no fork, and each answer runs the plain chain with a line that says to
  restart the agent. Only Linux has the parent-death signal: elsewhere, macOS
  included, `fork-exec` refuses and `ClaudeForks` takes no fork, so every answer runs the plain
  chain, with the reason.
- What the forks do is written to `run-ahead.log` in the plugin's state directory, and
  `reviewer-control stats` says whether it pays ([statistics](explore-statistics.md)).
- Run-ahead stops for the round after `FAILURES_TO_HALT` failures in a row, with one notice
  that names the last: a fork that ends without a turn, a check at the answer that could not
  be made, a failed switch, and a refusal to take forks where run-ahead itself failed, counted
  once per question (an agent or a platform it never forks, `ForkHost::unforkable`, is none).
  Only a prepared turn used starts the count again, not a fork that kept a turn: otherwise a
  switch that fails on every question would never stop it.

## Guarantees across crates

- A fork stays in the reviewer's process group, so the hang-up of the pane's terminal reaches
  it too: never give it a process group or a session of its own.
- A reopened reviewer discards the forks a stopped reviewer left in the rounds of every review
  of the repository (`ReviewStore::rounds_with_forks`; forks of a reviewer that still runs stay
  its own). A question that forks of another running reviewer answer is not forked again: the
  record is checked under its lock before the forks start.
- A fork whose session may be the agent's is never discarded nor cleaned up.
- No prompt reaches the agent while it switches or settles: the session holds
  `PromptSender::hold`, which keeps the thread service's courier, and so the round
  conversation's wakeups and the prompts to every other agent, from sending until the switch
  or the settling ends.
- A fork's guard (`claude-fork`'s `PreToolUse` hook) refuses every jj command without
  `--ignore-working-copy`, so the jj commands the Explore instructions name
  (`crates/review-explore-runner/src/rules/sources-jj.md`) must keep that option.
- Claude Code runs a fork's shell commands in the user's shell, Bash or Zsh, with the user's
  options: the guard must read a command as both would.
- The agent and the round name each answer alike: while a question waits, the session reserves
  a request ID and an answer ID per choice it forks, tells them to every fork of that choice,
  forks taken again included, and saves a new answer that picks the choice under them, with a
  comment too. The pane builds its turns with IDs of its own, so it hears of its post as it
  posted it (`ExplorePosted::request`) and adopts the saved round. A Cancel answer or a
  reconsideration after a prepared turn then names the answer as the agent knows it.

## How Herdr and Claude Code behave

- Herdr's `pane.agent_status_changed` subscription tells when the agent is idle
  (`HerdrClient::subscribe_agent_status` holds once it returns; `AgentStatuses::forward`
  reports the current status, then each change); nothing polls. An `EventCanceller` ends the
  stream at once by shutting its socket down.
- After `/resume`, the switch waits for the plugin's `SessionStart` hook with the source
  `resume` and the fork's session, which comes about 0.5 s after Enter (Claude Code 2.1.292).
  Herdr hears of the session from its own hook, which Claude Code runs beside the reviewer's,
  and sends no event when a session changes (checked in Herdr 0.9.3: a session report emits
  `pane.updated` only when the agent's name changes), so the switch then asks Herdr every
  100 ms until it reports the agent on that session, idle. Both waits end 20 s after the
  `/resume`. `claude_fork::ForkWaits` holds these durations, the 10 s wait for a submit's
  answer and the 1 s before a dropped status watch subscribes again; the reviewer's
  `RunAheadSetup` passes the defaults, and lets a test pass small ones.
- Text left in Claude Code's input box joins a typed `/resume` into one prompt, `<text>
  /resume <session>`, which fires `UserPromptSubmit`; a command alone fires none. So while a
  switch or a settling waits for the session, the reviewer blocks every prompt of that pane,
  and the switch fails, the answer waiting for Retry. A blocked prompt runs no turn: Claude
  Code shows the reason and the original prompt in its transcript, and 2.1.309 puts the
  prompt back in the input box (2.1.292 left it empty), so a Retry joins it again until the
  user clears the box. A hook that gets no answer in time lets the prompt go on. The thread
  service confirms its own prompts through the same hook, and the same listener
  ([threads](threads.md#delivery)): a pane holds a prompt's expectation and a resume's at once.
- Settling rests on Claude Code taking what is typed while it loads a session after it, in
  order: once a settling `/resume` is typed and Herdr reports the agent there, an earlier late
  resume cannot move it. The contract check checks it (below).
- The test stand-in for Claude Code reports a resumed session to Herdr as Herdr's own hook
  does, with a newer `--seq` and `--session-start-source resume`; without them Herdr kept
  reporting the first session.

## The reviewer's Claude Code plugin

[ADR 0005](../adr/0005-hear-the-agent-through-a-claude-code-plugin.md) says why run-ahead hears
the agent through it; the `Makefile`'s `install` and `uninstall` say how it is installed. What
Claude Code 2.1.292 does with it:

- `claude plugin marketplace add` and `claude plugin install` succeed when they ran before.
  Run from another checkout, they move the marketplace and the `control` option to it.
- A plugin of a directory marketplace loads in place from the checkout: an edit of its hooks
  takes effect at the next session start, without a reinstall.
- A hook's command may hold `${user_config.control}` only in exec form (`command` and `args`);
  in a shell string, Claude Code refuses it. A hook gets `CLAUDE_PLUGIN_ROOT` and the
  options as `CLAUDE_PLUGIN_OPTION_<NAME>` too.
- To try the plugin with a real `claude` without installing it, run `claude --plugin-dir
  crates/claude-hooks/plugin --settings '{"pluginConfigs":{"progressive-reviewer@inline":{"options":{"control":"<reviewer-control>"}}}}'`,
  with no `HERDR_*` variable of the user's Herdr, `HERDR_PANE_ID` and `HERDR_SOCKET_PATH` of
  your own, and a scratch `XDG_RUNTIME_DIR`. It creates
  `~/.claude/plugins/data/progressive-reviewer-inline`, which you can delete. Without a login,
  `claude` runs no hook, so a scratch `CLAUDE_CONFIG_DIR` can check the install but not the
  hooks.
- The hooks reach a reviewer only when `XDG_RUNTIME_DIR` is set, in the agent's pane and in
  the reviewer's.

## The contract check

The stand-in plays Claude Code in the run-ahead tests, so they hold only while Claude Code
behaves as the stand-in assumes. The bodies of `crates/reviewer/src/runtime/run_ahead/contract.tests.rs`
check those assumptions, each step naming the one it rests on (`Assumption`): the fork's command
line, a typed `/resume` that Herdr follows, typed input taken in order, the transcripts' place.
Each body runs twice:

- its rehearsal (`rehearse_*`) runs the stand-in, in `make check`: it finds the faults of the
  test itself, its assertions, timing and input box, at no cost; not a timing fault that only
  real Claude Code's pace brings out: the stand-in tells Herdr it works as soon as it reads a
  prompt, so a missing wait for that passes the rehearsal;
- its contract check (`real_claude_code_*`), ignored by default, runs real Claude Code in the
  isolated Herdr's pane, at `claude-sonnet-5-5`, about ten turns on the subscription of the
  token's account. A failure names the assumption that broke, and prints the agent's pane.

The contract check has not had a real run yet: its first needs a token from
`claude setup-token`. Until that run, the rehearsals are its only proof.

Run the contract check once after an upgrade of Claude Code or of Herdr, or a change to the
fork's command line (`claude-fork`'s `arguments.rs`, `pane.rs`, `host.rs`) or to the switch and
the settling (`claude-fork`'s `switch.rs`, the session's `run_ahead/switch.rs` and
`run_ahead/settle.rs`): one run, not one per ticket. The rehearsals guard the tests the rest of
the time.

```sh
claude setup-token   # once: a token of the account whose subscription pays
export CLAUDE_CODE_OAUTH_TOKEN=<the token>
cargo build -p reviewer --bin reviewer-control -p review-mcp-config --bin reviewer-mcp -p stand-in-agent
cargo nextest run -p reviewer real_claude_code --run-ignored only
```

The agent and its forks log in with that token and run in a configuration of their own, under
the test's Herdr directory and gone with it (`CLAUDE_CONFIG_DIR`, and `XDG_CACHE_HOME`, where
Claude Code keeps its MCP servers' logs): the check never reads nor writes `~/.claude` nor
`~/.cache`. Never copy the owner's credentials there. They take none of the test's `CLAUDE*`
and `ANTHROPIC*` variables but the token: when the check runs under Claude Code, those of that
session would load its plugins from `~/.claude` and reach the session. The configuration gets Herdr's session
hook (`herdr integration install claude`), the reviewer's plugin with this build's
`reviewer-control` (`claude plugin install`), the stand-in's observing hooks
([test stand-ins](test-stand-ins.md)), and the permissions of an Explore turn ("don't ask", the
submit tools). What it still touches is the owner's `claude` program, run as it is, with its
updater off, the token's account, and `~/.local/share/applications/claude-code-url-handler.desktop`,
which Claude Code writes when it is missing.

The check sets in `.claude.json` what skips Claude Code 2.1.310's first-run screens in a new
configuration directory: `hasCompletedOnboarding`, a theme, the project's
`hasTrustDialogAccepted` and `hasCompletedProjectOnboarding`, and `hasSeenAutoDefaultNudge` and
`hasSeenAutoModeEntryWarning`. Without `hasSeenAutoDefaultNudge`, the first prompt typed with
"don't ask" on opens a dialog that offers auto mode, and that dialog takes the prompt and its
Enter.
