# Explore agent contract

What the agent's Explore prompts and the two Explore tools guarantee across files: why their contents are shaped as they are, and what a submission commits to. The authoritative wording of the agent's instructions is in `crates/review-explore-runner/src/` (`interview.md`, `wakeup.md` and their siblings).

## The prompts

- The tools advertise their full input schemas, derived from the shared submission types.
  The kickoff explains the review behavior without duplicating schema examples.
- The texts of earlier decisions in a kickoff are quoted line by line with `> `, as data
  rather than instructions. Their answer IDs are those rounds': no interpretation or agenda
  change of the new round can name them, and the agent names such a decision by its
  subject, never by a question number.
- After a Reset, the kickoff tells the agent that if its agent session still holds the
  reset rounds, their questions, answers, decisions, agreed tasks and question numbers are
  void.
- The reviewer's screens number a round's questions Q1, Q2 and so on. Question IDs name the
  subject, never a number.
- There is no tool to fetch an answer or a round, and no file carries one.

## A round with a Challenger

The protocol does not change: the agent alone calls the tools, and the Challenger only
returns text to it. An agent that can continue a subagent keeps the same Challenger for the
round; one that cannot has it keep a handoff file in the system temporary directory.

## Sources and citations

- Git's `checkpoint.review_unit` identifies the base tree; jj's `checkpoint.checkpoint`
  identifies the reviewed commit. Untracked files that Git diff omits are part of the
  change; jj merge bases use the merged parent tree.
- Citations never mark lines reviewed.
- A destination a language server leads to is read only inside the repository root.
- Historical evidence outside the diff hunks uses a native view of the full base text,
  and cannot send old coordinates to the live language server.

## Review marks

The reviewer applies a question turn's marks when the reviewer answers that question, so
review progress moves on the reviewer's action, and a conclusion's marks when it is accepted.
A prompt whose unreviewed diffs cannot be written,
including when the repository cannot be read, is not sent.

## Recommendations

A choice's `text` never says that the choice is recommended: the recommendation goes only in
its `recommendation` field, which the Explore page hides until the reviewer's first pick on a
question whose Door is not two-way.

## Submitting a turn

- The protocol validates the integrity of references, not the agent's reasoning. It cannot
  establish mechanically whether free text is agreement, or whether an assessment is true:
  the visible replies and recaps, the evidence, follow-up answers and the reviewer's
  inspection remain necessary.
- The durable `instance` is distinct from renewable `review` access. Access is never
  saved with the round. Reopening rotates it; the next explicit reviewer action supplies
  current access through the existing wakeup.
- Each call checks the pinned native agent session, then validates against the latest
  stored round under its lock, atomically saves the update and deduplication record,
  publishes it to the UI, and waits for UI application before acknowledging it.
- Validation errors leave the request open for repair; transport retries must reuse the
  identical semantic payload (with current `review` access after reconnection). A response
  saved before a lost acknowledgement is restored locally; it is not regenerated. Accepted
  retries return `accepted: true, applied: false`. Cancelled, obsolete or changed accepted
  payloads are rejected.

## The conclusion

Submitting a conclusion does not start implementation. Only `to_be_implemented` seeds the
editable task box, and the reviewer's **Implement** action sends the edited box contents,
authorizing those tasks and their validation. Delivery confirmation means the request was
sent, not that implementation has finished.
