# Run-ahead

What run-ahead costs and needs, the guarantees that span its crates, how Claude Code and Herdr behave under it, and how to run its real Claude Code tests.

Run-ahead forks the pane agent's session while a question waits (setting `RunAhead` in
[`crates/review-explore-round-settings`](../../crates/review-explore-round-settings), pane key
`z`). A bare answer continues as the fork of its choice, when that fork submitted its turn;
any other answer runs the plain chain. The module headers of `agent-fork`, `claude-fork`,
`review-run-ahead` and `crates/review-explore-session/src/run_ahead.rs` say how each part works.

## Cost and needs

- Run-ahead is experimental: the turns it prepares can be wasted when the reviewer talks with
  the agent in the chat while a question waits.
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
- Cancel answer after a prepared turn: the agent knows that answer by the ID its fork was
  told, so the next prompts name it so in their `Cancelled answer:` lines
  (`RoundForks::answer_as_told`).

## How Herdr and Claude Code behave

- Herdr's `pane.agent_status_changed` subscription tells when the agent is idle
  (`HerdrClient::forward_agent_status_while` reports the current status once subscribed, then
  each change); nothing polls.
- Herdr sends no event when a session changes, so the switch asks Herdr every 100 ms, for at
  most 20 s, until it reports the agent on the fork's session, idle, with an empty box.
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
