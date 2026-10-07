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
  without a `--settings` of its own. Only Linux has the parent-death signal: elsewhere, macOS
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
- Herdr sends no event when a session changes (checked in Herdr 0.9.3: a session report emits
  `pane.updated` only when the agent's name changes), so the switch asks Herdr every 100 ms,
  for at most 20 s, until it reports the agent on the fork's session, idle, with an empty box.
  `claude_fork::ForkWaits` holds these durations, the 10 s wait for a submit's answer and the
  1 s before a dropped status watch subscribes again; the reviewer's `RunAheadSetup` passes
  the defaults, and lets a test pass small ones.
- Settling rests on Claude Code taking what is typed while it loads a session after it, in
  order: once a settling `/resume` is typed and Herdr reports the agent there, an earlier late
  resume cannot move it. This is not checked.
- The test stand-in for Claude Code reports a resumed session to Herdr as Claude Code's session
  hook does, with a newer `--seq` and `--session-start-source resume`; without them Herdr kept
  reporting the first session.

## Tests with a real Claude Code

Two tests in `reviewer` run real Claude Code turns on your subscription, at
`claude-sonnet-5-5`, and are ignored by default:

```sh
cargo build -p reviewer --bin reviewer-control -p review-mcp-config --bin reviewer-mcp
cargo nextest run -p reviewer a_real_claude_code --run-ignored only
```

They write the agent's and the forks' transcripts in your Claude Code configuration and delete
them at the end.
