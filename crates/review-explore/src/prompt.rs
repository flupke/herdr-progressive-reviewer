//! Compact agent input; full questions and answers remain in reviewer history.
use crate::{ReviewerAnswer, TopicStatus, TurnRequest};
use review_guide::ReviewCheckpoint;
use serde::Serialize;

#[derive(Serialize)]
struct PromptTurn<'a> {
    instance: &'a str,
    request: &'a str,
    checkpoint: &'a ReviewCheckpoint,
    answer: Option<PromptAnswer<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_error: Option<&'a str>,
}

#[derive(Serialize)]
struct PromptAnswer<'a> {
    id: &'a str,
    question: Option<QuestionIdentity<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    option: Option<SelectedOption<'a>>,
    #[serde(skip_serializing_if = "str::is_empty")]
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    in_reply_to: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    corrects: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deferred: Option<bool>,
}

#[derive(Serialize)]
struct QuestionIdentity<'a> {
    id: &'a str,
    version: u32,
}

#[derive(Serialize)]
struct SelectedOption<'a> {
    id: &'a str,
    text: &'a str,
    outcome: TopicStatus,
}

impl<'a> From<&'a ReviewerAnswer> for PromptAnswer<'a> {
    fn from(answer: &'a ReviewerAnswer) -> Self {
        Self {
            id: &answer.id,
            question: answer.question.as_ref().map(|question| QuestionIdentity {
                id: &question.id,
                version: question.version,
            }),
            option: answer.option.as_ref().map(|option| SelectedOption {
                id: &option.id,
                text: &option.text,
                outcome: option.outcome,
            }),
            text: &answer.text,
            in_reply_to: answer
                .question
                .is_none()
                .then_some(answer.in_reply_to.as_str()),
            corrects: answer.corrects.as_deref(),
            deferred: answer.deferred.then_some(true),
        }
    }
}

impl TurnRequest {
    /// Reference the agent's original question without retransmitting its investigation.
    pub fn prompt_input(&self) -> impl Serialize + '_ {
        PromptTurn {
            instance: &self.instance,
            request: &self.request,
            checkpoint: &self.checkpoint,
            answer: self.answer.as_ref().map(PromptAnswer::from),
            response_error: self.response_error.as_deref(),
        }
    }
}
