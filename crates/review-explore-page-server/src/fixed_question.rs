//! The questions and the conclusion the standalone server's agent posts.

use review_explore::{Alternative, Conclusion, EvidenceRef, Question, SourceSide, TopicStatus};
use review_explore_page::RoundStage;

use crate::{cited_code, fixed_explanation};

/// The round's question `number`, from 1: the fixed questions in turn.
pub(crate) fn question_stage(number: usize) -> RoundStage {
    let question = if number % 2 == 1 {
        keep_draft()
    } else {
        draft_storage()
    };
    let citations = question.evidence.iter().map(cited_code::cite).collect();
    RoundStage::Question {
        number,
        question: Box::new(question),
        citations,
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

/// Cites the new `reopen`, then the round as it was before the change.
fn keep_draft() -> Question {
    Question {
        rationale: Some(fixed_explanation::RATIONALE.into()),
        visual: Some(fixed_explanation::VISUAL.into()),
        assessments: Some(fixed_explanation::assessments()),
        ..question(
            "keep-draft",
            "Should a reopened round keep the reviewer's unsent draft?",
            vec![
                alternative("keep", "Keep the draft", TopicStatus::Accepted),
                alternative("discard", "Discard the draft", TopicStatus::NeedsFollowUp),
            ],
            vec![
                cited_code::evidence(
                    SourceSide::New,
                    Some((7, 8)),
                    "reopen() no longer clears the draft: this is the decision.",
                ),
                cited_code::evidence(
                    SourceSide::Old,
                    Some((1, 3)),
                    "The round had no draft before the change.",
                ),
            ],
        )
    }
}

/// Cites the whole file.
fn draft_storage() -> Question {
    question(
        "draft-storage",
        "Where should the kept draft be stored?",
        vec![
            alternative("round", "In the round's record", TopicStatus::Accepted),
            alternative("editor", "In the editor state", TopicStatus::NeedsFollowUp),
        ],
        vec![cited_code::evidence(
            SourceSide::New,
            None,
            "Everything the round saves is in this file.",
        )],
    )
}

fn question(
    id: &str,
    text: &str,
    alternatives: Vec<Alternative>,
    evidence: Vec<EvidenceRef>,
) -> Question {
    Question {
        id: id.into(),
        version: 1,
        topic: "drafts".into(),
        text: text.into(),
        rationale: None,
        visual: None,
        alternatives,
        evidence,
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
