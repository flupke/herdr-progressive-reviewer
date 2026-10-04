//! The round rail: the steps of the round, and which are done, current and later.

use serde::Serialize;

use super::quiz::QuizStage;
use super::steps::{Activity, Current, Position, QuestionStep};
use crate::Exploration;

/// One step of the round rail and its state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RailStep {
    pub step: Step,
    pub state: StepState,
}

/// A step of the round: the design, a question, the quiz or the conclusion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    /// The design explanation. A round saved before the first turn explained the design has
    /// no such step.
    Design,
    /// A question the agent posted, numbered from 1, with its clarified versions. The rail
    /// lists the questions posted so far: the round does not know how many come.
    Question { number: usize },
    /// The conclusion's quiz. Before the conclusion the rail shows it as a later step; a
    /// conclusion without a quiz has no such step.
    Quiz { stage: QuizStage },
    /// The conclusion, the round's last step.
    Conclusion,
}

/// Where a step stands in the round. The rail tells the round's state, not the screen the
/// reviewer looks at: while the round waits for an answer to question 1, Design is done and
/// question 1 current, and the page shows that the reviewer reads the design.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepState {
    Done,
    /// The step the round stands at.
    Current {
        /// The agent works on the turn after the step ("Q2 · working"). False while the round
        /// waits for the reviewer, and while a turn waits for Retry.
        working: bool,
    },
    Later,
}

impl RailStep {
    /// The rail of `exploration`, with its question steps `steps`, standing at `position`.
    pub(super) fn rail(
        exploration: &Exploration,
        steps: &[QuestionStep<'_>],
        position: &Position<'_>,
    ) -> Vec<Self> {
        let step = |step: Step, at: Current, done: bool| Self {
            step,
            state: StepState::at(position, at, done),
        };
        let mut rail = Vec::new();
        if exploration.design().is_some() || exploration.conversation.is_empty() {
            rail.push(step(Step::Design, Current::Design, true));
        }
        rail.extend(steps.iter().map(|question| {
            let number = question.number;
            step(Step::Question { number }, Current::Question(number), true)
        }));
        if let Some(quiz) = Step::quiz(position) {
            rail.push(step(quiz, Current::Quiz, position.concluded()));
        }
        rail.push(step(Step::Conclusion, Current::Conclusion, false));
        rail
    }
}

impl StepState {
    /// The state of the step `step` in a round standing at `position`, when the step is
    /// `done` unless it is current.
    fn at(position: &Position<'_>, step: Current, done: bool) -> Self {
        if position.current == step {
            Self::Current {
                working: position.activity == Activity::Working,
            }
        } else if done {
            Self::Done
        } else {
            Self::Later
        }
    }
}

impl Step {
    /// The quiz step of a round standing at `position`: a later step before the conclusion,
    /// none for a conclusion without a quiz.
    fn quiz(position: &Position<'_>) -> Option<Self> {
        let stage = if position.concluded() {
            position.quiz()?.stage()
        } else {
            QuizStage::Later
        };
        Some(Self::Quiz { stage })
    }
}
