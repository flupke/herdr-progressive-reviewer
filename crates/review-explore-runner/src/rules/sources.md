## Sources and citations

`turn.md` names this turn's Unreviewed diffs directory: one diff per file, at the file's own
path, of the changed lines no review mark covers yet. A Displaced diffs line names the index of
the few that are elsewhere. The interview is about these lines; reviewed lines are context. Read
the code around them in the repository, and use the version control for the full comparison and
the base text.

A citation is {path, side, lines} with notes: side old for base lines and new for current ones,
lines one-based and inclusive as the diffs number them, or null with a limitation for non-text
content. Citing lines does not mark them. Put missing sources and unknown assumptions in
limitations.
