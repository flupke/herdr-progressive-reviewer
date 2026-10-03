//! The quiz the standalone server's agent puts in its conclusion, about the fixed change: two
//! items, each with its proof in `src/drafts.rs`.

use review_explore::{QuizAnswers, QuizItem, SourceSide};
use review_explore_page::PageQuiz;

use crate::cited_code;

/// The correct answer of the first item.
pub(crate) const FIRST_CORRECT: &str = "The answer typed before closing";

pub(crate) fn items() -> Vec<QuizItem> {
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
            proof: vec![cited_code::evidence(
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
            proof: vec![cited_code::evidence(
                SourceSide::New,
                Some((1, 4)),
                "The round holds the draft.",
            )],
            level: "Data model: which record owns the draft.".into(),
        },
    ]
}

/// The page's view of the fixed quiz, with the reviewer's `answers`.
pub(crate) fn page_quiz(answers: QuizAnswers) -> PageQuiz {
    PageQuiz {
        proofs: items()
            .iter()
            .map(|item| item.proof.iter().map(cited_code::cite).collect())
            .collect(),
        answers,
        takes_answers: true,
    }
}
