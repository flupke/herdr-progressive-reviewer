# Back to the drawing board

The reviewer disagrees with the change's design and sends you back to the drawing board, in
place of an answer to the question `turn.md` names. The code changes before another round
reviews it, so this turn ends the round with the changes the design needs. A Reply to
conclusion in `turn.md` answers your conclusion at the drawing board: conclude again, with what
it changes.

## Steps

1. Disregard each Cancelled answer and your turn after it, as if neither happened.
2. Gather what the round said against the design: the reviewer's answers and comments, and
   the round conversation's messages. Read the source until you can say what each change
   touches.
3. Conclude with submit_conclusion:
   - `summary`: what the design gets wrong, and the design it moves to, in a few plain
     sentences, with a diagram when the structure changes (see `diagrams.md`). Keep what the
     reviewer asked for apart from what you propose on top.
   - `to_be_implemented`: the changes, one per line, each with what to change and why, then
     the tasks earlier answers agreed that the code still lacks. In a review of someone
     else's change, write them as review comments for its author. The reviewer edits this
     list before acting on it.
   - `future_work`: what can wait for a later change. Empty when nothing can.
   - `quiz` empty, and `quiz_empty_reason` saying that the quiz waits for the new design.
   - `interpretation` null, and `reviewed`, `reopened` and `not_relevant` empty: the code will
     change.

The challenger takes no part in this turn. Once the changes are in, the reviewer starts a new
round on them.
