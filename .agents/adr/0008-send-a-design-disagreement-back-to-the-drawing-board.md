# Answer a disagreement with the design by sending the agent back to the drawing board

Reviewers often disagree with a change right at its design explanation, before the first
question. The agent could only explain the design on the first turn and correct it in a reply;
the round then went on questioning code whose architecture the reviewer had already disagreed with.

The reviewer can now answer the question that waits with Back to the drawing board, from the
design screen, under the answer or in the masthead's menu. The agent concludes at once, with the
changes the design needs as the list to implement, no quiz and no review marks; the reviewer
edits the list and implements it, then starts a new round on the new code. The answer is a
`ReviewerAnswer` with `drawing_board` set, so Cancel answer, Retry and Stop waiting work as for
any answer, and the tool refuses any other turn than such a conclusion.

Implementing that conclusion reopens every line the round marked reviewed, the lines its turns
took over from another author included: those decisions were made for the rejected design,
and the new round reviews them again. It happens at Implement rather than when the reviewer
sends the agent back, since Cancel answer is offered until a request is made, and nothing then
has to mark the lines again.

Considered:
- Let the agent post the design again, from the chat or on a later turn, as a revised
  question replaces the open one: a disagreement with the architecture is not fixed by
  explaining the same code again, and questions about code that will change waste the
  reviewer's time. An explanation that is only wrong is still corrected in a reply.
- Post the new design as a new round, keeping the earlier one as history: the new design
  describes code that does not exist yet, which the round's questions and quiz cannot cite.
- A comment asking the agent to conclude: it depends on the agent reading it so, and the
  conclusion it gets would carry a quiz and marks for code about to change.
- Keep the marks of the round's earlier answers, and review only the lines the new code
  rewrites: lines left as they were would keep a decision made for the rejected design.
- Reopen the marks when the reviewer sends the agent back: cancelling that answer would then
  have to mark the lines again, which Cancel answer cannot, since they belong to earlier
  answers.
