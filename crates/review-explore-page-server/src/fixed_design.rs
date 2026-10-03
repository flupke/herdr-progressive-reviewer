//! The design of the change that the standalone server's round explains on its first turn: a
//! table with status marks and a callout among its parts.

use review_explore::Design;

pub(crate) fn design() -> Design {
    Design {
        overview: "\
The change keeps the reviewer's unsent draft when a round is reopened. It adds a draft field to \
the saved editor state and restores it when the pane rebuilds its editor."
            .into(),
        data_flow: "\
The draft is saved with the editor state, in the round record.

| Step | Holds the draft |
| --- | --- |
| The reviewer types | [!good] The editor |
| The round is saved | [!good] The editor state |
| The reviewer reopens | [!good] The rebuilt editor |"
            .into(),
        algorithm: "\
One more string is written with each save of the editor state: no extra read or write.

> [!WARNING]
> A long draft makes every save of the editor state larger."
            .into(),
        alternatives: "\
Saving the draft in its own file was rejected: a second file could fall out of step with the \
round."
            .into(),
    }
}
