## Questions

Ask only what the reviewer decides or alone knows: settle the facts yourself, from the code, its
tests and its history. Write for a reviewer who has not read the implementation, at the depth
their answers show.

- `text`: the decision, in one sentence.
- `alternatives`: brief and distinct. Recommend one, its reason in its `recommendation`; the
  choices' texts stay neutral. When the change needs a fix, recommend the smallest one. Offer a
  choice with outcome open for context only the reviewer has. The screens add None of the
  above.
- `rationale`, the Context: open with the scenario, a short numbered list of what the users or
  the parts of the system do, with concrete values, ending on the result the question is about;
  draw it as a sequence diagram when several actors interleave. Then name the lines of this
  change that raise the question, and the exact decision. For a behavior change, say what
  changes, then why: the problem, what goes wrong without the change, and whether a source
  states the reason or you infer it. The how comes last.
- `evidence`: a few cited snippets, most decisive first, each a distinct fact that could change
  the answer, its notes saying why it matters.
- `assessments`, on a consequential question: Door, how hard the decision is to reverse, judged
  on its consequences rather than on a revert; Blast radius, what could fail, how far and how
  badly; a rollback does not undo harm done. Back each with evidence, and put counts,
  likelihoods and recovery times that no evidence supports in the unknowns.

A question belongs to a topic that is open or flagged for reconsideration, and whose
prerequisites are understood; let the irreversible or broad effects still unresolved choose it. A
posted question is fixed: clarify it with a higher version of its ID, and give a new question a
new ID that names its subject. The screens number the round's questions Q1, Q2 and so on:
submit_question returns the number in `shown_as`, and a later `turn.md` gives it as Shown as.
Name a question by its number in every text.
