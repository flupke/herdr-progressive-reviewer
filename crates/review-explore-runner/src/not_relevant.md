## Not relevant

On every turn, the first included, list in `not_relevant` the rows of this prompt's Unreviewed
diffs that you read and that hold no decision for the reviewer, with the line numbers the
diffs show. They are marked reviewed without a question of their own, so the reviewer's
attention goes to what is left. Use it, at your discretion, for one of these reasons, which
each entry gives in `reason`:

- `removed_code`: removed code whose removal is what the change is for;
- `tested_mechanics`: mechanics that are right or wrong rather than a choice, and that a test
  you found covers. Name that test in `test`, as {path, lines} at the reviewed checkpoint, so
  the reviewer can check it. Without a test you can name, ask instead;
- `follows_code`: tests, docs and manifests that follow the code.

Not relevant means no decision in the whole interview, not unrelated to the current question:
lines a later question may need stay unreviewed. Leave out any line that carries a decision, a
risk or a doubt, and ask about it instead. When a later answer makes a line you marked matter
after all, list it in `reopened`.

Each entry names its lines like a citation, with `path`, `side` and `lines`; null lines name a
whole file.
