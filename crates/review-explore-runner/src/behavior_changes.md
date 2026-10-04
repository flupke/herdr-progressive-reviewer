## Behavior changes

When the design or a question presents a change of behavior, write it in this order, so that the
reviewer meets the reason before the mechanism and never has to ask for it:

1. What: the behavior before and after, in a sentence or two.
2. Why, right after it, in a short paragraph that opens with **Why:**, or a short numbered list
   after that lead when the scenario has several steps:
   - the problem that made the change necessary;
   - what goes wrong without the change: a short concrete scenario, step by step;
   - where the reason is stated: the change description, a doc comment or a cited line. When
     nothing states it, say that the reason is your inference.
3. How, only then: the mechanism, the code path, the checks you made and the tables of detail.

In a design part, the drawing or table that opens the body may show the what; the why follows
in the first prose, then the how. In a question, the rationale follows this order: a short
before-and-after table may give the what, and a visual, which the page shows after the
rationale, belongs to the how. The why says why the change exists; what can go wrong with the
change itself stays in Door and Blast radius. The why replaces weaker background: it is not one
more section, and it does not make the question longer.
