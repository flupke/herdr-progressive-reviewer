//! The questions and the conclusion the standalone server's agent posts.

use review_explore::{Alternative, Conclusion, Question, TopicStatus};
use review_explore_page::RoundStage;

/// The round's question `number`, from 1: the fixed questions in turn.
pub(crate) fn question_stage(number: usize) -> RoundStage {
    let question = if number % 2 == 1 {
        keep_draft()
    } else {
        draft_storage()
    };
    RoundStage::Question {
        number,
        question: Box::new(question),
    }
}

/// The fixed conclusion.
pub(crate) fn conclusion_stage() -> RoundStage {
    RoundStage::Conclusion(Box::new(Conclusion {
        summary: "A reopened round keeps the reviewer's unsent draft.".into(),
        to_be_implemented: "Save the draft with the round.".into(),
        future_work: "Offer to discard an old draft.".into(),
    }))
}

fn keep_draft() -> Question {
    question(
        "keep-draft",
        "Should a reopened round keep the reviewer's unsent draft?",
        vec![
            alternative("keep", "Keep the draft", TopicStatus::Accepted),
            alternative("discard", "Discard the draft", TopicStatus::NeedsFollowUp),
        ],
    )
}

fn draft_storage() -> Question {
    question(
        "draft-storage",
        "Where should the kept draft be stored?",
        vec![
            alternative("round", "In the round's record", TopicStatus::Accepted),
            alternative("editor", "In the editor state", TopicStatus::NeedsFollowUp),
        ],
    )
}

fn question(id: &str, text: &str, alternatives: Vec<Alternative>) -> Question {
    Question {
        id: id.into(),
        version: 1,
        topic: "drafts".into(),
        text: text.into(),
        rationale: None,
        visual: None,
        alternatives,
        evidence: Vec::new(),
        assessments: None,
    }
}

fn alternative(id: &str, text: &str, outcome: TopicStatus) -> Alternative {
    Alternative {
        id: id.into(),
        text: text.into(),
        outcome,
        recommendation: None,
    }
}
