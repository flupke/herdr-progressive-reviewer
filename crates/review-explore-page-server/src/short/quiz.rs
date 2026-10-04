//! The quiz the standalone server's agent puts in its conclusion, about the fixed change: two
//! items, each with its proof in `src/drafts.rs`.

use review_explore::{QuizItem, SourceSide};

use super::change;

/// The correct answer of the first item.
pub(super) const FIRST_CORRECT: &str = "The answer typed before closing";

pub(super) fn items() -> Vec<QuizItem> {
    vec![
        QuizItem {
            question: "The reviewer types an answer, closes the pane, then opens the round again. \
                       What does the answer box show?"
                .into(),
            answers: vec![
                "An empty box".into(),
                FIRST_CORRECT.into(),
                "The agent's recommended choice".into(),
            ],
            correct: 1,
            why: "Reopening a round no longer clears the draft that the round keeps.".into(),
            proof: vec![change::DRAFTS.evidence(
                SourceSide::New,
                Some((7, 8)),
                "reopen() leaves the draft alone.",
            )],
            level: "Data storage: what survives closing the pane.".into(),
        },
        QuizItem {
            question: "Where does the kept draft live while the pane is closed?".into(),
            answers: vec![
                "With the round's record".into(),
                "In the pane's editor".into(),
            ],
            correct: 0,
            why: "The draft is a field of the round, saved wherever the round is saved.".into(),
            proof: vec![change::DRAFTS.evidence(
                SourceSide::New,
                Some((1, 4)),
                "The round holds the draft.",
            )],
            level: "Data model: which record owns the draft.".into(),
        },
    ]
}
