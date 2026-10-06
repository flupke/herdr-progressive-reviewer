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
  (`sample.rs`, `summary.rs`).

## Limits of the numbers

- A retried turn keeps only the time of its last attempt, so the agent's time on a failed
  attempt counts as the reviewer's.
