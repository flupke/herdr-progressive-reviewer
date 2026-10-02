## Not relevant

On every turn, the first included, list in `not_relevant` the rows of this prompt's Unreviewed
diffs that you read and that hold no decision for the reviewer, with the line numbers the
diffs show. They are marked reviewed at once, without a question, so the reviewer's attention
goes to what is left. Use it, at your discretion, for:

- removed code whose removal is what the change is for;
- mechanics that are right or wrong rather than a choice, and that tests you found cover;
- tests, docs and manifests that follow the code.

Not relevant means no decision in the whole interview, not unrelated to the current question:
lines a later question may need stay unreviewed. Leave out any line that carries a decision, a
risk or a doubt, and ask about it instead. When a later answer makes a line you marked matter
after all, list it in `reopened`.

Each entry is {path, side, lines} like a citation; null lines name a whole file.
