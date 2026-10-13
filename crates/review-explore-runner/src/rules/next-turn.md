# Next turn

`turn.md` brings the reviewer's answer. It answers the question whose ID and version it names,
as you posted it earlier in this conversation, or the conclusion a Reply to conclusion names.

## Steps

1. Disregard each Cancelled answer and your turn after it, as if neither happened: the reviewer
   has already reversed the review marks that answer applied, and the answer in `turn.md`
   replaces it.
2. Investigate in source what the answer changes, until you can say what it settles and what
   it leaves open. A question about the code is answered by inspecting the source.
3. Interpret the answer (see Interpretation) and respond to it directly: in reply on a question
   turn, in summary when concluding. Cite evidence for what you say about the code.
4. Mark the lines the answer settled (see Review marks).
5. Update the agenda: add, refine or reorder topics as understanding changes (see
   Agenda changes). Then ask the next question with submit_question, or conclude (see
   `conclusion.md`).

## Interpretation

An answer may be a decision, a question, a challenge, context or a redirection. A decision or
material agreement gets an interpretation, naming the exact Answer ID:

- An unqualified selected option keeps its stated outcome.
- A comment adding conditions or requested changes makes it needs_follow_up, with each one in
  follow_ups. Nonempty follow_ups requires needs_follow_up.
- Material but ambiguous agreement stays open: ask one focused clarification.
- Context, factual questions, context answers and silence are not acceptance, and
  interpretation is null; context can still change the agenda. A Reply to conclusion also has
  a null interpretation: respond to that earlier conclusion, with a new question when the reply
  warrants one.

No Comment section means no comment; no Selected option section means no option was chosen.
The recap is shown to the reviewer, who amends it by answering again. If the source contradicts
what the reviewer says, show the conflict and ask a focused follow-up.

## Review marks

Record what the answer settled in the same submit_question or submit_conclusion:

- `reviewed`: rows of this turn's Unreviewed diffs that the answer settled, cited or not,
  with the line numbers the diffs show: lines the reviewer decided, accepted, or now
  understands well enough that no question about them remains, including lines a requested
  change will rewrite. Null lines mark a whole file.
- `reopened`: reviewed lines the answer makes matter again, whoever marked them. They are no
  longer in the diffs: take their numbers from the repository.
- `not_relevant`: see Not relevant in the index.

Lines the answer did not settle stay unmarked. A question's marks apply when the reviewer
answers it, a conclusion's when it is accepted; the next turn's Unreviewed diffs show the
result.

## Agenda changes

Only an interpretation changes a topic's status: a topic you send keeps the status the reviewer
gave it. Verify in source before dropping a risk. Agenda operations take a reason
backed by an Answer ID or source evidence:

- retire only a topic that depended on an invalidated premise; independent risks survive, and
  retirement does not cascade.
- supersede names an active replacement topic and keeps the original wording and decisions.
- reconsider flags a decision of this round without changing it, naming its Answer ID as decision
  when one exists. A later decision by the reviewer resolves it.

The latest operation on a topic defines its lifecycle. After a topic is decided, move to
another.
