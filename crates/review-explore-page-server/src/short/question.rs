//! The questions and the conclusion of the short data set.

use review_explore::{
    Conclusion, Interpretation, NotRelevantMark, NotRelevantReason, Question, SourceSide,
    TopicStatus,
};
use review_explore_page::{QuestionMarks, TurnResponse};

use super::{change, diagrams, explanation, quiz};
use crate::question_parts;

/// The fixed question `number`, from 1, in turn, and the lines an answer to it marks: an
/// answer to the first fixed question marks lines.
pub(super) fn question(number: usize) -> (Question, QuestionMarks) {
    if number % 2 == 1 {
        (keep_draft(), keep_draft_marks())
    } else {
        (draft_storage(), QuestionMarks::default())
    }
}

/// What the agent says back to the reviewer's previous answer before its next question.
pub(super) fn answer_response() -> TurnResponse {
    TurnResponse {
        interpretations: vec![interpretation(
            "Recorded: **keep** the draft.",
            vec!["Say when a kept draft is older than a day.".into()],
        )],
        reply: Some("Agreed: the draft stays with the round, so `reopen()` keeps it.".into()),
    }
}

/// The agent's interpretation of the reviewer's previous answer.
fn interpretation(recap: &str, follow_ups: Vec<String>) -> Interpretation {
    Interpretation {
        answer: "previous-answer".into(),
        status: if follow_ups.is_empty() {
            TopicStatus::Accepted
        } else {
            TopicStatus::NeedsFollowUp
        },
        recap: recap.into(),
        follow_ups,
    }
}

/// The fixed conclusion: with the fixed quiz when `quiz`, or else with the reason it has none.
pub(super) fn conclusion(quiz: bool) -> Conclusion {
    Conclusion {
        summary: "A reopened round keeps the reviewer's unsent draft.".into(),
        to_be_implemented: "Save the draft with the round.".into(),
        future_work: "Offer to discard an old draft.".into(),
        quiz: if quiz { quiz::items() } else { Vec::new() },
        quiz_empty_reason: (!quiz)
            .then(|| "The change only keeps a field that existed already.".into()),
    }
}

/// What the agent says back to the reviewer's last answer above its conclusion.
pub(super) fn conclusion_response() -> TurnResponse {
    TurnResponse {
        interpretations: vec![interpretation(
            "Recorded: store the draft in the round's record.",
            Vec::new(),
        )],
        reply: None,
    }
}

/// Cites the new `reopen`, then the round as it was before the change.
fn keep_draft() -> Question {
    Question {
        rationale: Some(explanation::RATIONALE.into()),
        visual: Some(explanation::VISUAL.into()),
        assessments: Some(explanation::assessments()),
        ..question_parts::question(
            "keep-draft",
            "drafts",
            "Should a reopened round keep the reviewer's unsent draft?",
            vec![
                question_parts::alternative("keep", "Keep the draft", TopicStatus::Accepted),
                question_parts::alternative(
                    "discard",
                    "Discard the draft",
                    TopicStatus::NeedsFollowUp,
                ),
            ],
            vec![
                change::DRAFTS.evidence(
                    SourceSide::New,
                    Some((7, 8)),
                    "reopen() no longer clears the draft: this is the decision.",
                ),
                change::DRAFTS.evidence(
                    SourceSide::Old,
                    Some((1, 3)),
                    "The round had no draft before the change.",
                ),
            ],
        )
    }
}

/// Four lines reviewed and twenty not relevant.
fn keep_draft_marks() -> QuestionMarks {
    QuestionMarks {
        reviewed: vec![question_parts::lines("src/drafts.rs", 10, 13)],
        not_relevant: vec![NotRelevantMark {
            location: question_parts::lines("tests/drafts.rs", 1, 20),
            reason: Some(NotRelevantReason::FollowsCode),
            test: None,
        }],
        reopened: Vec::new(),
    }
}

/// Cites the whole file.
fn draft_storage() -> Question {
    Question {
        rationale: Some(diagrams::RATIONALE.into()),
        ..question_parts::question(
            "draft-storage",
            "drafts",
            "Where should the kept draft be stored?",
            vec![
                question_parts::alternative(
                    "round",
                    "In the round's record",
                    TopicStatus::Accepted,
                ),
                question_parts::alternative(
                    "editor",
                    "In the editor state",
                    TopicStatus::NeedsFollowUp,
                ),
            ],
            vec![change::DRAFTS.evidence(
                SourceSide::New,
                None,
                "Everything the round saves is in this file.",
            )],
        )
    }
}
