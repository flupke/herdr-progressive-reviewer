//! The reviewer's answer to a question step, and the decisions of a round.

use serde::Serialize;
use ts_rs::TS;

use super::steps::QuestionStep;
use crate::{Alternative, Exploration, ReviewerAnswer};

/// The answer the reviewer kept on a question: the latest answer to its latest version.
///
/// "None of the above" is a choice: `choice` names it, and it is never "as recommended". A
/// comment-only answer has no `choice`, only its `comment`, and no tag unless the reviewer
/// had made a first pick.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, TS)]
pub struct KeptAnswer {
    /// The text of the choice the reviewer sent; `None` for a comment-only answer.
    pub choice: Option<String>,
    /// The reviewer's comment; empty when there is none.
    pub comment: String,
    /// Each tag that applies, in the order of [`DecisionTag`]: none, one, or both when the
    /// reviewer moved to the recommendation after a first pick of another choice.
    pub tags: Vec<DecisionTag>,
}

/// How the kept choice relates to the reviewer's first pick and to the agent's recommendation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DecisionTag {
    /// "changed after your first pick": the reviewer's saved first pick is not the kept
    /// choice.
    ChangedAfterFirstPick,
    /// "as recommended": the kept choice is one the agent recommended.
    AsRecommended,
}

impl KeptAnswer {
    pub(super) fn of(answer: &ReviewerAnswer) -> Self {
        Self::new(
            answer.option.as_ref(),
            &answer.text,
            answer.first_pick.as_deref(),
        )
    }

    /// The answer that kept the choice `kept`, if any, with the comment `comment`, after the
    /// reviewer's first pick `first_pick`, by the choice's ID, on a question that hid its
    /// recommendation until then.
    pub fn new(kept: Option<&Alternative>, comment: &str, first_pick: Option<&str>) -> Self {
        let changed = first_pick.is_some_and(|first| kept.is_none_or(|kept| kept.id != first));
        let recommended = kept.is_some_and(|kept| kept.recommendation.is_some());
        Self {
            choice: kept.map(|kept| kept.text.clone()),
            comment: comment.to_owned(),
            tags: [
                changed.then_some(DecisionTag::ChangedAfterFirstPick),
                recommended.then_some(DecisionTag::AsRecommended),
            ]
            .into_iter()
            .flatten()
            .collect(),
        }
    }
}

/// One line of "Your decisions": a question the reviewer answered, and the answer kept.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Decision {
    /// The question's number in the rail.
    pub number: usize,
    /// The text of the question's latest version.
    pub question: String,
    pub answer: KeptAnswer,
}

impl Decision {
    /// The reviewer's decisions in `exploration`: one for each question step whose latest
    /// version was answered, in the order of the rail.
    pub fn of_round(exploration: &Exploration) -> Vec<Self> {
        QuestionStep::of(exploration)
            .iter()
            .filter_map(|step| Self::of(exploration, step))
            .collect()
    }

    /// The decision of `step`, once the reviewer answered its latest version.
    pub(super) fn of(exploration: &Exploration, step: &QuestionStep<'_>) -> Option<Self> {
        let answer = step.answer(exploration)?;
        Some(Self {
            number: step.number,
            question: step.question().text.clone(),
            answer: KeptAnswer::of(answer),
        })
    }
}
