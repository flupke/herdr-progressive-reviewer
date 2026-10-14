# Text selection

Where the text of a text selection comes from, and the guess that joins wrapped prose.

The reviewer captures the mouse, so the terminal's own selection never reaches the pane.
A left drag selects text instead, and copies it on release. Where a plain drag already means
something, the text selection needs Alt held: a drag over the code of the diff, of its peek or
of one of Explore's evidence viewers comments on lines. The threads drawn under that code, and
the thread conversation view, take a plain drag, since a drag means nothing to them. Alt works
in every pane; Shift and a drag stay the outer terminal's own selection. Only the press needs Alt: a terminal reports the modifier on every
move of the drag, or not, as it likes.

Each text region says which press starts a text selection in it (`TextRegion::needing_alt`),
and a plain press over a region that needs Alt starts none, whatever lies under it. So a
region that covers a drag handle must need Alt: Explore puts one under each evidence split,
whose divider and bottom row resize it. Regions nest the other way too: a diff viewer's region
needs Alt and holds all its rows, and each run of thread rows lies over it in a plain region
of its own. A text selection stays in the region it started in, so a plain drag copies within
one thread, and an Alt drag from the code can run across the threads under it. Pane borders, where the other dividers are, lie
outside every region. A plain press that starts a text selection still reaches the
application as a click, and so does its release: clicking a file or a button still works.

## Where the text comes from

Each frame lays out the text regions it drew, later ones on top, and a text selection stays
in the region it started in, copying that frame's snapshot of it. Code gives its exact text
(the diff and Explore's evidence viewers); everything else is read back from the screen
inside its borders, as prose where a pane wraps words.

## Joining wrapped prose

The Markdown renderer (`vendor/ratatui-markdown`) and Ratatui's word wrapper return rows,
not where they broke a line, and both drop the space they broke at. So prose is joined by
running the wrapping's own test backwards (`TextRegion::push_prose`).

The guess fails where a hard line break falls where the next word would not have fitted,
which Markdown paragraphs do not produce. Exact soft wraps would need the vendored
renderer to report them through each of its wrapping paths.

## Known gaps

- A blockquote's `│ ` prefix starts each of its rows, so a wrapped quote is copied as rows,
  bar included.
- The snapshot is the press's: content that moves during the drag, such as an agent's text
  arriving, leaves the highlight and the copy on the old cells.
- A toast is drawn after the regions are read: selecting over one copies what it hides.
- A text selection started beside an open modal is read from under it: dragging under the
  modal copies what it hides, and paints over it.
- The thread conversation view's code is read back as drawn, each row a line: its tabs
  copy as spaces.
