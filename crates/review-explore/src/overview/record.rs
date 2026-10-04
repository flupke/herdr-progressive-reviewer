//! The record of an earlier question, to show read only.

use serde::Serialize;

use super::answer::KeptAnswer;
use super::steps::QuestionStep;
use crate::{ExploreRound, Interpretation, Question, TurnMarks};

/// A question step the round went past, as the reviewer answered it and the agent recorded it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EarlierQuestion {
    /// The question's number in the rail.
    pub number: usize,
    /// The question's latest version, as the reviewer answered it.
    pub question: Question,
    /// The reviewer's answer to it; `None` for a question the round left unanswered, which only
    /// a free-text reply to another question in the pane can do.
    pub answer: Option<KeptAnswer>,
    /// What the agent recorded of the answer.
    pub recorded: AgentRecord,
    /// The review marks the reviewer's answers to the step's versions applied, in the order the
    /// agent's turns asked for them: those of the turn that posted each version, applied when
    /// the reviewer answered it, then those of a conclusion that followed the answer, applied
    /// when the round accepted it. Usually one; a clarified question may have one per version.
    pub marks: Vec<TurnMarks>,
}

/// What the agent recorded of an answer: how it interpreted it, and its reply.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct AgentRecord {
    /// The agent's latest interpretation of the answer, with its recap and follow-ups; `None`
    /// when it gave none, as for context it took without a decision.
    pub interpretation: Option<Interpretation>,
    /// The reply of the agent's turn that took up the answer, in Markdown.
    pub reply: Option<String>,
}

impl EarlierQuestion {
    pub(super) fn of(round: &ExploreRound, step: &QuestionStep<'_>) -> Self {
        let exploration = &round.exploration;
        let answer = step.answer(exploration);
        Self {
            number: step.number,
            question: step.question().clone(),
            answer: answer.map(KeptAnswer::of),
            recorded: answer.map_or_else(AgentRecord::default, |answer| AgentRecord {
                interpretation: exploration.interpretation(&answer.id).cloned(),
                reply: exploration
                    .turn_after(&answer.id)
                    .and_then(|turn| turn.update.reply.as_ref())
                    .map(|reply| reply.text.clone()),
            }),
            marks: round
                .marks_applied_by(|answer| step.answered_by(answer))
                .cloned()
                .collect(),
        }
    }
}
