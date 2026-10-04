//! The explanation of the first fixed question: a table with status marks, a callout, a sketch,
//! raw HTML that must show as text, and the Door and Blast radius sections.

use review_explore::{Assessments, Consequence, Door};

pub(super) const RATIONALE: &str = "\
When the reviewer reopens a round, the pane rebuilds its editor from the saved round. The \
unsent draft lives only in memory today, so a reopen loses it.

| Choice | Survives a reopen | Cost |
| --- | --- | --- |
| Keep the draft | [!good] Yes | [!warning] One more field in the record |
| Discard the draft | [!bad] No | [!good] None |

> [!TIP]
> The draft is saved with the editor state, not with the answers, so keeping it changes no \
answer.

A draft keeps what the reviewer typed, such as <b>tags</b>, as plain text.";

pub(super) const VISUAL: &str = "\
```text
reopen ──▶ saved round ──▶ editor
               └─ draft (proposed)
```";

pub(super) fn assessments() -> Assessments {
    Assessments {
        door: Door::TwoWay,
        reversibility: Consequence {
            summary: "Removing the field later drops only unsent drafts.".into(),
            details: String::new(),
            evidence: Vec::new(),
            unknowns: Vec::new(),
        },
        blast_radius: Consequence {
            summary: "Only the reviewer's own unsent text is at stake.".into(),
            details: String::new(),
            evidence: Vec::new(),
            unknowns: vec!["How long drafts grow in long rounds.".into()],
        },
    }
}
