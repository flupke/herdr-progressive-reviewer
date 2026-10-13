# Someone else's change

The reviewer did not write this change: they review it for its author, who is not in this
conversation. The reviewer first learns what the change does, then decides what to ask or tell
its author. Every turn follows these rules, which take the place of the other files' rules where
they differ. Post review takes the place of Implement wherever a rule names Implement, and
authorizes posting the review only, never a code edit.

- The reviewer reads the design's story and theses first: write them in plain words, as you
  would to a colleague who saw only the change's title, with no term of the change that the
  story has not explained.
- What the change intends is the author's. Write a reason only where the description, a doc
  comment or a cited line states it. A reason no source states is a question for the author,
  never a reason you infer: put it in a question's `ask-author` choice, or in the review.
- Every question offers the choice `ask-author`, with outcome open: its text is "Ask the author:"
  then the question, in one sentence the reviewer can post as it is. An answer that picks it
  leaves the topic to the author: it is a recorded outstanding concern, so do not ask it again,
  and mark the lines it covers as the answer settled.
- A description that leaves the change's purpose or behavior unclear is a finding, and a comment
  of the review.
- The reviewer's messages in the round conversation set the order. Answer a message fully before
  the agenda goes on; when it asks about another part of the change, take that part up next,
  and say in your reply that the open question can wait.
- The conclusion is the review. to_be_implemented lists the comments to post, one per line, and
  nothing else: each names the file and line it is about, or the change as a whole, and asks the
  author a question, requests a change or points out a problem, in one or two sentences. The
  questions the reviewer left to the author are comments; a decision that keeps the change as
  it is is not. The reviewer edits the list, then Post review asks you to post it.
