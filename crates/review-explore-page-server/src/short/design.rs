//! The design of the change that the standalone server's round explains on its first turn: a
//! sequence diagram, a table with status marks and a callout among its parts, each as wide as an
//! agent's explanation tends to be: the diagram and the table fit a desktop window, not the
//! width of a phone.

use review_explore::{Design, DesignPart};

pub(super) fn design() -> Design {
    Design::new(
        "Reopening a round brings back the reviewer's unsent draft, saved with the editor state in the round record.",
        DesignPart::new(
            "The change keeps the reviewer's unsent draft when a round is reopened.",
            "\
It adds a draft field to the saved editor state and restores it when the pane rebuilds its \
editor.

Nothing else moves: the answers, the marks and the round's history are saved as before, and a \
round saved before the change opens with an empty draft.",
        ),
        DesignPart::new(
            "The draft is saved with the editor state, in the round record.",
            "\
```mermaid
sequenceDiagram
  participant reviewer as Reviewer
  participant pane as Pane
  participant state as Editor state
  participant record as Round record
  reviewer->>pane: types a comment, sends nothing
  pane->>state: keeps the draft with each keystroke
  pane->>record: saves the editor state with the round
  reviewer->>pane: reopens the round the next day
  record-->>pane: the saved editor state, with its draft
  pane->>state: rebuilds the editor from it
  state-->>reviewer: shows the unsent draft again
```

| Step | Holds the draft | Saved where |
| --- | --- | --- |
| The reviewer types a comment | [!good] The editor, in memory | [!warning] Nowhere yet: a crash before the next save loses the last keystrokes |
| The round is saved | [!good] The editor state | [!good] The round record, beside the answers, in the same write |
| The reviewer reopens the round | [!good] The rebuilt editor | [!good] Read back from the round record; a record without the field gives an empty draft |",
        ),
        DesignPart::new(
            "One more string is written with each save of the editor state: no extra read or write.",
            "\
> [!WARNING]
> A long draft makes every save of the editor state larger.",
        ),
        DesignPart::new(
            "Saving the draft in its own file was rejected.",
            "\
A second file could fall out of step with the round.",
        ),
    )
}
