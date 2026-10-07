# Run-ahead

What run-ahead costs and needs, the guarantees that span its crates, how Claude Code and Herdr behave under it, and how to run its real Claude Code tests.

Run-ahead forks the pane agent's session while a question waits (setting `RunAhead` in
[`crates/review-explore-round-settings`](../../crates/review-explore-round-settings), pane key
`z`). A bare answer continues as the fork of its choice, when that fork submitted its turn;
any other answer runs the plain chain. The module headers of `agent-fork`, `claude-fork`,
`review-run-ahead` and `crates/review-explore-session/src/run_ahead.rs` say how each part works.

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
  (below): the switch waits for them. Only Linux has the parent-death signal: elsewhere, macOS
  included, `fork-exec` refuses and `ClaudeForks` takes no fork, so every answer runs the plain
  chain, with the reason.
- What the forks do is written to `run-ahead.log` in the plugin's state directory.

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
  `--ignore-working-copy`, so the jj commands the Explore prompt names
  (`crates/review-explore-runner/src/interview.md`) must keep that option.
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
- Settling rests on Claude Code taking what is typed while it loads a session after it, in
  order: once a settling `/resume` is typed and Herdr reports the agent there, an earlier late
  resume cannot move it. This is not checked.
- The test stand-in for Claude Code runs the plugin's hook as Claude Code does,
  `reviewer-control agent-hook` with Claude Code's event on its standard input, then reports a
  resumed session to Herdr as Herdr's own hook does, with a newer `--seq` and
  `--session-start-source resume`; without them Herdr kept reporting the first session.

## The reviewer's Claude Code plugin

[ADR 0005](../adr/0005-hear-the-agent-through-a-claude-code-plugin.md) says why run-ahead hears
the agent through it. `make install` runs, against the user's Claude Code configuration:

```sh
claude plugin marketplace add "$PWD/crates/claude-hooks"
claude plugin install progressive-reviewer@herdr-progressive-reviewer --config control="$PWD/bin/reviewer-control"
```

Both say so and succeed when they ran before; `make uninstall` runs `claude plugin uninstall`
and `claude plugin marketplace remove`. What Claude Code 2.1.292 does with it:

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

## Tests with a real Claude Code

Two tests in `reviewer` run real Claude Code turns on your subscription, at
`claude-sonnet-5-5`, and are ignored by default:

```sh
cargo build -p reviewer --bin reviewer-control -p review-mcp-config --bin reviewer-mcp
cargo nextest run -p reviewer a_real_claude_code --run-ignored only
```

They write the agent's and the forks' transcripts in your Claude Code configuration and delete
them at the end.
