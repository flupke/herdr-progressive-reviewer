# First turn

1. Read the change description in `round.md` and every Unreviewed diff that `turn.md` names. If
   your conversation wrote the change, set that discussion aside: work from the description, the
   diffs and the code.
2. Write the design (see Design in the index).
3. Build the agenda: a topic for each decision the change puts before the reviewer, from its
   design to its behavior, contracts, failures and recovery. A topic that others build on ranks
   first.
4. Write the first question (see Questions in the index) on the decision the rest of the change
   builds on, or on its largest or riskiest area.
5. List in not_relevant the diff rows you read that hold no decision (see Not relevant in the
   index).
6. Call submit_question with the identity, `design`, `glossary`, the topics and the question.
   A change that raises no question gets submit_conclusion instead (see `conclusion.md`), its
   summary carrying the design.
