//! The Explore prompts as a vision session's turn log records them (crate `vision-turns`): what
//! a reply must carry, and the answer each prompt brings.

use review_explore::{ReviewerAnswer, TopicStatus, TurnRequest};
use review_source::ReviewCheckpoint;
use serde::Serialize;

/// One prompt as it was sent.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SentTurn {
    kind: TurnKind,
    /// What a reply must carry; an Implement prompt takes no reply.
    #[serde(flatten)]
    identity: Option<TurnIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    answer: Option<SentAnswer>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cancelled: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unreviewed: Option<String>,
    text: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum TurnKind {
    Kickoff,
    Wakeup,
    Implement,
}

#[derive(Clone, Debug, Serialize)]
struct TurnIdentity {
    access: String,
    round: String,
    request: String,
    checkpoint: ReviewCheckpoint,
}

/// The reviewer's answer as the prompt states it.
#[derive(Clone, Debug, Serialize)]
struct SentAnswer {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    question: Option<SentQuestion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_conclusion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    option: Option<SentOption>,
    text: String,
}

#[derive(Clone, Debug, Serialize)]
struct SentQuestion {
    id: String,
    version: u32,
}

#[derive(Clone, Debug, Serialize)]
struct SentOption {
    id: String,
    text: String,
    outcome: TopicStatus,
}

impl SentAnswer {
    fn new(answer: &ReviewerAnswer) -> Self {
        Self {
            id: answer.id.clone(),
            question: answer.question.as_ref().map(|question| SentQuestion {
                id: question.id.clone(),
                version: question.version,
            }),
            reply_to_conclusion: answer
                .question
                .is_none()
                .then(|| answer.in_reply_to.clone()),
            option: answer.option.as_ref().map(|option| SentOption {
                id: option.id.clone(),
                text: option.text.clone(),
                outcome: option.outcome,
            }),
            text: answer.text.clone(),
        }
    }
}

impl SentTurn {
    /// An interview prompt: the kickoff, or the wakeup after an answer.
    pub(crate) fn interview(
        request: &TurnRequest,
        access: &str,
        unreviewed: String,
        text: String,
    ) -> Self {
        Self {
            kind: if request.answer.is_some() {
                TurnKind::Wakeup
            } else {
                TurnKind::Kickoff
            },
            identity: Some(TurnIdentity {
                access: access.to_owned(),
                round: request.instance.clone(),
                request: request.request.clone(),
                checkpoint: request.checkpoint.clone(),
            }),
            answer: request.answer.as_ref().map(SentAnswer::new),
            cancelled: request.cancelled.clone(),
            unreviewed: Some(unreviewed),
            text,
        }
    }

    pub(crate) fn implement(text: String) -> Self {
        Self {
            kind: TurnKind::Implement,
            identity: None,
            answer: None,
            cancelled: Vec::new(),
            unreviewed: None,
            text,
        }
    }
}
