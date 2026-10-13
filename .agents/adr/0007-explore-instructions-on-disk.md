# Give the Explore agent its instructions as files the review tool writes for each turn

Each Explore prompt carried all of the agent's rules: about 25k characters on the first turn,
whatever the round's setup, and a reminder of them on every later turn. The agent wrote the way
the prompt read, long and detailed, and it could not tell which rules the turn in front of it
needed.

The review tool now writes each turn's instructions into a private temporary directory, and the
prompt only names the round, the directory's `index.md` and the tools that end the turn. The
index lists the turn's data (`turn.md`: identity, access, the answer), the round's (`round.md`:
the change and what the review decided before, on every turn), then only the rule files the
round's setup needs: the first turn's steps or a later turn's, the challenger's script and the
rules for someone else's change when the round has them, and the rules only some turns need
(diagrams, the conclusion, the round conversation). It says which to read now and when to read
each of the others. The rules that every turn needs, in the writing style and with the commands
of the change's version control, are in the index itself. The directory lives until the next
prompt; each fork gets its own, and the switch to a fork prepares the real turn with the fork's
diffs and compares the two turns' files.

Considered:
- A skill, which the agent loads when it judges it useful: it is installed apart from the
  review tool, so it can lag the binary, as an MCP server started before an install did when it
  could not send a glossary; it serves Claude Code alone, while rounds also run on Codex; and a
  rule every turn needs would depend on the agent choosing to load it.
- A shorter prompt that keeps the rules inline: it still sends every branch's rules to every
  round, and leaves no place for rules a turn needs only at one step.
- Files in the review tool's state directory, kept for the whole round: they would outlive a crash
  with the access value in them, and a restart writes every prompt anew anyway.
