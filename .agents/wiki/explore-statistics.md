# Explore statistics

What `reviewer-control stats` reads and how it counts each row. It exists to tell whether a change to Explore (a prompt change, the Challenger, a new page) helps.

```sh
bin/reviewer-control stats
bin/reviewer-control stats --since 2026-09-15 --until 2026-09-30
```

## What it reads

- The command reads the rounds saved for the checkout it runs in; each jj workspace keeps
  its own rounds. It only reads them.
- It reads Herdr's state directory for the plugin,
  `$XDG_STATE_HOME/herdr/plugins/herdr.progressive-reviewer` (by default under
  `~/.local/state`), or `HERDR_PLUGIN_STATE_DIR` when set.
- It prints one table for all rounds and, with `--since` or `--until`, a second one for the
  rounds started in that period. A date covers that whole day in local time; an RFC 3339
  time such as `2026-09-15T14:00:00+02:00` is exact, and `--until` excludes it.
- Each table has a column for all rounds, one for rounds with a Challenger and one for
  rounds without.

## The rows

- The number of rounds, and how many have answers and a conclusion.
- The questions per round (median and range), over the rounds with an answer.
- The share of answers that asked for a change: those the agent interpreted as needing a
  follow-up, out of the answers its next turn took up. An answer the agent left
  uninterpreted, as it may for free text, asked for nothing.
- The share of answers that did not choose the recommended choice, out of the answers to
  questions that recommended one. An answer in free text alone did not choose it.
- The median time of an agent turn, over every turn and over the turns after an answer.
- The median time the reviewer took to answer, from the agent's question to the answer.
- The median share of a round's time (agent turns plus answers) spent waiting for the
  agent.
- The median number of words the agent wrote for a question: its text, context, sketch,
  choices with their recommendations, evidence notes, assessments, and the reply above it.
- Over the rounds with a Challenger: the number of questions the Challenger proposed, the
  number of rounds that reported any, and how many were asked, merged with the agent's own
  question, retired by a fact, or kept for a later turn. A proposal that several turns
  report under the same title counts once, at the result of the latest turn.

## Edge cases

- A round with no answer counts as a round and stays out of the numbers about answers.
- The command says how many saved rounds it could not read: rounds saved by earlier
  versions, and damaged records.
- A round whose turns carry no time, saved before turns were timed, counts in the period
  by the time its file was last saved.
- A retried turn keeps only the time of its last attempt, so the agent's time on a failed
  attempt counts as the reviewer's.
- A round saved before the agent reported proposals has none, so it is not among the
  rounds that reported proposals.
