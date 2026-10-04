## Quiz

The reviewer did not write this change but must still be able to explain how the system works
after it, as they would at a whiteboard interview. submit_conclusion carries a short quiz that
checks this, in `quiz`. Write it last, about the code at the checkpoint, which includes the
round's agreed tasks only once they are in the code. Where the reviewer follows the round on
the Explore page, the page asks the quiz before the summary and grades each pick itself; the
pane does not ask it. Either way, no answer comes back to you.

What a question may test, and only where this change creates or alters it:

- architecture: which components exist, which one does what, and how data and control pass
  between them, across which boundary (thread, process, disk, agent);
- algorithms: the steps a procedure takes, in what order, and what decides each branch;
- data storage: what is kept, where, in what form, when it is written, read back or removed,
  and what survives a restart;
- data model: the records and states the system holds, how they relate, and which invariants
  hold between them.

Never test a detail of the implementation: names of functions, types, files, fields and flags;
signatures; the logic inside one function; error text; tests, docs and build files; UI wording
and layout. Ask about the normal path. An edge case (a name clash, a crash, a race, a rare
sequence of user actions) qualifies only when handling it is the purpose of the design. This
holds for the round's own decisions too: an edge case the round settled, or asked to fix, is
still an edge case. A rule for a failure qualifies only when the data model keeps it as a
state or a record, such as a request saved as possibly sent; a branch that only reports or
retries is a detail, even when the round decided it.

How many: as few as possible, at most three. The first question covers the change's central
idea, the first thing a colleague would draw when explaining it; add another only for a
second, independent fact of the same weight. A change confined to one component's behavior
gets one question about the state model behind it, or none; a component is what a colleague
would draw as one box: a crate, a service, a store, or one screen with its own state. A change
with nothing at this level (a removal, a rename or a move, a UI or wording change, a fix inside
one function, tests or docs alone) gets an empty quiz and one sentence saying why in
`quiz_empty_reason`, which stays null when the quiz has items. An empty quiz is a correct
answer, never fill it. A fact that is the absence of code, such as data a removal leaves
behind, has no lines to prove it: leave it out.

Each item:

- `question`: a short concrete scenario in the running system, at most about 40 words, that a
  person who understood the design but never read the code answers correctly. No code
  identifiers; example paths and values are fine.
- `answers`: two to four options of similar length and form, each under about 20 words. Each is
  something the system could credibly do; wrong options are what a reader who misunderstood the
  design would believe, such as the behavior before the change or a nearby alternative design.
- `correct`: the zero-based index of the correct option. Vary its position across items.
- `why`: one sentence that explains the correct option in design terms.
- `proof`: citations of the lines that establish the correct option, most decisive first, each
  {path, side, lines} with notes like any citation, its lines never null. Cite the new side; add
  old-side lines only to show the behavior a wrong option describes. A test may follow the code
  as supporting proof, never stand alone.
- `level`: one sentence naming the whiteboard topic it tests and why it is not an
  implementation detail. The reviewer does not see it.

Check each correct option against the source before submitting, and drop an item that the
source does not settle.
