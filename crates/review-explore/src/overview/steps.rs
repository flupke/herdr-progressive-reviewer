//! The question steps of a round, and where the round stands among its steps.

use super::quiz::Quiz;
use crate::{Conclusion, Exploration, Question, QuizAnswers, ReviewerAnswer};

/// One question of the round rail: the versions of one question that the agent posted one
/// after the other. A clarification is a higher version of the question that follows the
/// earlier one directly, so it stays in the earlier one's step. A version posted after another
/// question came back to a question the reviewer had left: it is a step of its own, in the
/// order the reviewer met it.
pub(super) struct QuestionStep<'a> {
    /// The step's number in the rail, from 1.
    pub(super) number: usize,
    versions: &'a [Question],
}

impl<'a> QuestionStep<'a> {
    /// The question steps of `exploration`, in the order the agent posted them.
    pub(super) fn of(exploration: &'a Exploration) -> Vec<Self> {
        exploration
            .questions
            .chunk_by(|earlier, later| earlier.id == later.id)
            .enumerate()
            .map(|(index, versions)| Self {
                number: index + 1,
                versions,
            })
            .collect()
    }

    /// The step's latest version.
    pub(super) fn question(&self) -> &'a Question {
        self.versions.last().expect("a step has a version")
    }

    /// The reviewer's latest answer to the step's latest version.
    pub(super) fn answer(&self, exploration: &'a Exploration) -> Option<&'a ReviewerAnswer> {
        let question = self.question();
        exploration
            .answers
            .iter()
            .rev()
            .find(|answer| answer.answers(question))
    }

    /// Whether `answer` answers one of the step's versions.
    pub(super) fn answered_by(&self, answer: &ReviewerAnswer) -> bool {
        self.versions
            .iter()
            .any(|question| answer.answers(question))
    }
}

/// What the agent does with the turn the round waits for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Activity {
    /// The agent works on a turn the session delivers.
    Working,
    /// A turn waits for Retry: its prompt failed, the reviewer stopped waiting, or the reviewer
    /// reopened during it.
    Interrupted,
    /// No turn is pending: the round waits for the reviewer.
    Idle,
}

/// What the agent's latest turn posted.
enum Ending<'a> {
    /// No turn yet: the round waits for its first.
    NoTurn,
    /// A question, the round's latest step.
    Asking,
    /// A conclusion, with its quiz and what the reviewer answered of it.
    Concluded {
        conclusion: &'a Conclusion,
        answers: Option<&'a QuizAnswers>,
    },
}

/// The step the round stands at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Current {
    Design,
    Question(usize),
    Quiz,
    Conclusion,
}

/// Where the round stands: what the agent does, what its latest turn posted, and the step
/// that makes current.
pub(super) struct Position<'a> {
    pub(super) activity: Activity,
    ending: Ending<'a>,
    pub(super) current: Current,
}

impl<'a> Position<'a> {
    /// Where `exploration`, with its question steps `steps`, stands; `delivering` as
    /// [`super::RoundOverview::of`] takes it.
    pub(super) fn of(
        exploration: &'a Exploration,
        steps: &[QuestionStep<'_>],
        delivering: Option<&str>,
    ) -> Self {
        let mut position = Self {
            activity: Self::activity(exploration, delivering),
            ending: Self::ending(exploration),
            current: Current::Design,
        };
        position.current = position.find_current(steps);
        position
    }

    fn activity(exploration: &Exploration, delivering: Option<&str>) -> Activity {
        match exploration.pending_request() {
            Some(pending) if delivering == Some(pending.request.as_str()) => Activity::Working,
            Some(_) => Activity::Interrupted,
            None if exploration.retry_request().is_some() => Activity::Interrupted,
            None => Activity::Idle,
        }
    }

    fn ending(exploration: &'a Exploration) -> Ending<'a> {
        let Some(turn) = exploration.conversation.last() else {
            return Ending::NoTurn;
        };
        match &turn.update.conclusion {
            Some(conclusion) => Ending::Concluded {
                conclusion,
                answers: exploration.quiz_answers(&turn.update.request),
            },
            None => Ending::Asking,
        }
    }

    /// The step the round stands at, among `steps`.
    fn find_current(&self, steps: &[QuestionStep<'_>]) -> Current {
        match &self.ending {
            Ending::NoTurn => Current::Design,
            Ending::Asking => Current::Question(steps.len()),
            Ending::Concluded { .. } if self.quiz().is_some_and(|quiz| quiz.running()) => {
                Current::Quiz
            }
            Ending::Concluded { .. } => Current::Conclusion,
        }
    }

    /// Whether the agent's latest turn posted a conclusion.
    pub(super) fn concluded(&self) -> bool {
        matches!(self.ending, Ending::Concluded { .. })
    }

    /// The conclusion's quiz, once the round concluded with one.
    pub(super) fn quiz(&self) -> Option<Quiz<'a>> {
        match &self.ending {
            Ending::Concluded {
                conclusion,
                answers,
            } if !conclusion.quiz.is_empty() => Some(Quiz {
                items: conclusion.quiz.len(),
                answers: *answers,
            }),
            _ => None,
        }
    }

    /// Whether the round went past the question step `step`.
    pub(super) fn passed(&self, step: &QuestionStep<'_>) -> bool {
        self.current != Current::Question(step.number)
    }
}
