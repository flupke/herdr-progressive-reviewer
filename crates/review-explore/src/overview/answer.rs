//! The reviewer's answer to a question step, and the decisions of a round.

use serde::Serialize;

use super::steps::QuestionStep;
use crate::{Exploration, ReviewerAnswer};

/// The answer the reviewer kept on a question: the latest answer to its latest version.
///
/// "None of the above" is a choice: `choice` names it, and it is never "as recommended". A
/// comment-only answer has no `choice`, only its `comment`, and no tag unless the reviewer
/// had made a first pick.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct KeptAnswer {
    /// The text of the choice the reviewer sent; `None` for a comment-only answer.
    pub choice: Option<String>,
    /// The reviewer's comment; empty when there is none.
    pub comment: String,
    pub tag: Option<DecisionTag>,
}

/// How the kept choice relates to the agent's recommendation and to the reviewer's first pick.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionTag {
    /// "as recommended": the kept choice is one the agent recommended.
    AsRecommended,
    /// "changed after your first pick": the reviewer's saved first pick is not the kept
    /// choice. It wins over "as recommended": a reviewer who moved to the recommendation after
    /// seeing it changed their pick.
    ChangedAfterFirstPick,
}

impl KeptAnswer {
    pub(super) fn of(answer: &ReviewerAnswer) -> Self {
        let kept = answer.option.as_ref();
        let changed = answer
            .first_pick
            .as_ref()
            .is_some_and(|first| kept.is_none_or(|kept| &kept.id != first));
        let recommended = kept.is_some_and(|kept| kept.recommendation.is_some());
        let tag = if changed {
            Some(DecisionTag::ChangedAfterFirstPick)
        } else if recommended {
            Some(DecisionTag::AsRecommended)
        } else {
            None
        };
        Self {
            choice: kept.map(|kept| kept.text.clone()),
            comment: answer.text.clone(),
            tag,
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
