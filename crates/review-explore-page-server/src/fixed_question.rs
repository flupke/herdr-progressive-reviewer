//! The questions and the conclusion the standalone server's agent posts.

use review_explore::{
    Alternative, CodeLocation, Conclusion, EvidenceRef, NotRelevantMark, NotRelevantReason,
    Question, SourceSide, TopicStatus,
};
use review_explore_page::{QuestionMarks, RoundStage};
use review_repository::repository::RepoPath;
use review_source::SourceLineRange;

use crate::{cited_code, fixed_diagrams, fixed_explanation};

/// The round's question `number`, from 1: `question` when given, which marks nothing, or else
/// the fixed questions in turn. An answer to the first fixed question marks lines.
pub(crate) fn question_stage(number: usize, question: Option<Question>) -> RoundStage {
    let (question, marks) = match question {
        Some(question) => (question, QuestionMarks::default()),
        None if number % 2 == 1 => (keep_draft(), keep_draft_marks()),
        None => (draft_storage(), QuestionMarks::default()),
    };
    let citations = question.evidence.iter().map(cited_code::cite).collect();
    RoundStage::Question {
        number,
        question: Box::new(question),
        citations,
        marks,
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

/// Four lines reviewed and twenty not relevant.
fn keep_draft_marks() -> QuestionMarks {
    QuestionMarks {
        reviewed: vec![lines("src/drafts.rs", 10, 13)],
        not_relevant: vec![NotRelevantMark {
            location: lines("tests/drafts.rs", 1, 20),
            reason: Some(NotRelevantReason::FollowsCode),
            test: None,
        }],
        reopened: Vec::new(),
    }
}

fn lines(path: &str, first_line: u32, last_line: u32) -> CodeLocation {
    CodeLocation {
        path: RepoPath::from_bytes(path.as_bytes()),
        side: SourceSide::New,
        lines: Some(SourceLineRange {
            first_line,
            last_line,
        }),
    }
}

/// Cites the whole file.
fn draft_storage() -> Question {
    Question {
        rationale: Some(fixed_diagrams::RATIONALE.into()),
        ..question(
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
