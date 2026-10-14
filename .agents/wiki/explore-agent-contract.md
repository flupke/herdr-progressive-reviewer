# Explore agent contract

What the agent's Explore prompts and the two Explore tools guarantee across files: why their contents are shaped as they are, and what a submission commits to. The authoritative wording of the agent's instructions is in `crates/review-explore-runner/src/rules/`, and which files a turn gets in `rules.rs` beside it.

## The prompts

- The tools advertise their full input schemas, derived from the shared submission types.
  The kickoff explains the review behavior without duplicating schema examples.
- Each turn's instructions are files, not the prompt ([ADR 0007](../adr/0007-explore-instructions-on-disk.md)):
  the prompt names the round, the turn's `index.md` and the submit tools. The index lists the
  turn's data (`turn.md`: the identity, the access, the answer), the round's (`round.md`: the
  change, its author, reset rounds, earlier decisions, reviewer notes, the same on every turn)
  and only the rule files the round's setup needs, the ones to read now and when to read each
  other one. The rules every turn needs (questions, sources, texts, not relevant, and the design
  on a first turn) are in the index itself, so that one read gives them. A rule for data that
  only some rounds carry is written beside that data in `round.md`. Field detail stays in the
  tools' schemas.
- The rules are a brief, not a rulebook: the agent writes as densely as its instructions read,
  and a 25k-character kickoff gave long, detailed designs. Cut a rule before adding one.
- A turn's directory is private (0700) and lives until the next prompt: the session keeps the
  main agent's, and each fork keeps its own, beside the diffs the forks share. A rule file does
  not change during a round, so a later turn's agent reads again only what it lost from its
  context.
- The texts of earlier decisions in `round.md` are quoted line by line with `> `, as data
  rather than instructions. Their answer IDs are those rounds': no interpretation or agenda
  change of the new round can name them, and the agent names such a decision by its
  subject, never by a question number.
- After a Reset, `round.md` tells the agent that if its agent session still holds the
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
- A topic's status is the reviewer's: the tools leave it out of their input, and a topic the
  agent sends keeps the status recorded for it, which only the interpretation of an answer
  changes. A settled topic cannot be rewritten.
- The glossary is the merge of every turn's `glossary`, a later definition of a term replacing
  the earlier one; Cancel answer rebuilds it from the turns that remain.
- A revised question (`revise_question`) is a turn of its own with no prompt behind it: it
  follows the same answer as the turn it revises, and carries that turn's reply and review
  marks, so the reviewer's answer applies those marks as it would have. Cancel answer drops
  it with that turn. It takes the Explore access of the latest Explore turn's `turn.md`, since the
  comments wakeup that brings the message carries the review threads' access.
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

Back to the drawing board ([ADR 0008](../adr/0008-send-a-design-disagreement-back-to-the-drawing-board.md))
is an answer to the question that waits, with `drawing_board` set and no choice: its turn gets
`drawing-board.md` in place of `next-turn.md` and `conclusion.md`, and the tool takes only a
conclusion with a list to implement, an empty quiz, no interpretation and no review marks. A
Reply to that conclusion, from the pane, stays at the drawing board under the same rules. The
answer applies none of the marks its question waited for, since the code will change, and
run-ahead's forks go unused (`PlainReason::DrawingBoard`). Implementing that conclusion, or
posting it as a review of someone else's change, reopens every line the round's turns marked,
and gives back none they took over
(`ExploreSession::reopen_round_marks`): only then, since Cancel answer is offered until a
request is made, and before the request is saved, so that a failure refuses Implement.
