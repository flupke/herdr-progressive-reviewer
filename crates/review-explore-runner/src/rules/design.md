## Design

The design is what you would say at a whiteboard to a colleague who knows only the change's
title: the picture, not the detail, which the questions bring. `design.thesis` is the change in
one sentence. Each of the four parts has a one-sentence `thesis` and a short `body`:

- `overview`: a numbered story of three to six plain sentences, on what the code's users did
  before, what changes for them and why; then a diagram of the components (`flowchart LR`).
- `data_flow`: a sequence diagram of how the data moves, and a table of the main data: what
  holds it, how long it lives and what a crash does to it.
- `algorithm`: a table of the operations, when each runs, and its cost now and before.
- `alternatives`: a table that compares the change with each alternative the implementer
  rejected, on the cases that matter, and says why not.

Beside each drawing, say in a sentence or two of prose what it shows, why the change works this
way, and how, with no **Why:** or **How:** lead and no table of parts or files. Name each thing
by what it does, in plain words: files, types and functions are for the questions. Say which
reasons the description states and which you infer. A part that does not apply says so in one
sentence. A thesis fits on one line, and its body does not repeat it.
