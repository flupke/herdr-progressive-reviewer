//! The question the standalone server's agent posts.

use review_explore::{Alternative, Question, TopicStatus};
use review_explore_page::RoundStage;

/// The fixed question, as the round's first question.
pub(crate) fn question_stage() -> RoundStage {
    RoundStage::Question {
        number: 1,
        question: Box::new(fixed_question()),
    }
}

fn fixed_question() -> Question {
    Question {
        id: "keep-draft".into(),
        version: 1,
        topic: "drafts".into(),
        text: "Should a reopened round keep the reviewer's unsent draft?".into(),
        rationale: None,
        visual: None,
        alternatives: vec![
            alternative("keep", "Keep the draft", TopicStatus::Accepted),
            alternative("discard", "Discard the draft", TopicStatus::NeedsFollowUp),
        ],
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
