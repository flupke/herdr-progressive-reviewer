# Explore statistics

How to run `reviewer-control stats` and the limits of its numbers. It exists to tell whether a change to Explore (a prompt change, the Challenger, a new page) helps.

```sh
bin/reviewer-control stats
bin/reviewer-control stats --since 2026-09-15 --until 2026-09-30
```

- The command reads the rounds saved for the checkout it runs in; each jj workspace keeps
  its own rounds. It only reads them.
- It reads Herdr's state directory for the plugin,
  `$XDG_STATE_HOME/herdr/plugins/herdr.progressive-reviewer` (by default under
  `~/.local/state`), or `HERDR_PLUGIN_STATE_DIR` when set.
- What each row counts is documented on the fields of `review-explore-stats`
  (`sample.rs`, `summary.rs`, `run_ahead.rs`). The run-ahead rows read the record saved beside
  each round (`<round>.forks.json`); a round without one, as every round saved before
  run-ahead, counts as having no run-ahead.

## Limits of the numbers

- A retried turn keeps only the time of its last attempt, so the agent's time on a failed
  attempt counts as the reviewer's.
- A revised question comes from the round conversation, with no prompt behind it: it adds no
  agent turn, and the reviewer's answer to it has no answer time, since no turn record says
  when the question was posted.
- Run-ahead counts only the answers to the questions it watched: none while it was off or
  stopped for the round.
- The wait after an answer runs from the answer to the end of its turn. A plain chain after a
  failed switch waits for Retry, so it counts the reviewer's time until Retry too.
- Whether a fork was ready when its question was answered is unknown for a fork that kept its
  turn in a record saved before forks kept when they submitted: such a fork is left out.
