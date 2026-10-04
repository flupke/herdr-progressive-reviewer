//! The question steps of a round, and where the round stands among its steps.

use super::quiz::QuizProgress;
use crate::{Exploration, Question, ReviewerAnswer};

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
pub enum Activity {
    /// The agent works on a turn the session delivers.
    Working,
    /// A turn waits for Retry: its prompt failed, the reviewer stopped waiting, or the reviewer
    /// reopened during it.
    Interrupted,
    /// No turn is pending: the round waits for the reviewer.
    Idle,
}

/// What the agent's latest turn posted.
#[derive(Clone, Copy, Debug)]
pub enum LatestTurn<'a> {
    /// No turn yet: the round waits for its first.
    None,
    /// A question, the round's latest step.
    Question,
    /// The conclusion, with its quiz and what the reviewer answered of it; `None` for a
    /// conclusion without a quiz.
    Conclusion { quiz: Option<QuizProgress<'a>> },
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
    latest: LatestTurn<'a>,
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
        Self::new(
            Self::activity(exploration, delivering),
            Self::latest(exploration),
            steps.len(),
        )
    }

    /// Where a round stands whose agent does `activity`, whose latest turn posted `latest`,
    /// with `questions` question steps.
    pub(super) fn new(activity: Activity, latest: LatestTurn<'a>, questions: usize) -> Self {
        let current = match latest {
            LatestTurn::None => Current::Design,
            LatestTurn::Question => Current::Question(questions),
            LatestTurn::Conclusion { quiz: Some(quiz) } if quiz.running() => Current::Quiz,
            LatestTurn::Conclusion { .. } => Current::Conclusion,
        };
        Self {
            activity,
            latest,
            current,
        }
    }

    fn activity(exploration: &Exploration, delivering: Option<&str>) -> Activity {
        match exploration.pending_request() {
            Some(pending) if delivering == Some(pending.request.as_str()) => Activity::Working,
            Some(_) => Activity::Interrupted,
            None if exploration.retry_request().is_some() => Activity::Interrupted,
            None => Activity::Idle,
        }
    }

    fn latest(exploration: &'a Exploration) -> LatestTurn<'a> {
        let Some(turn) = exploration.conversation.last() else {
            return LatestTurn::None;
        };
        match &turn.update.conclusion {
            Some(conclusion) => LatestTurn::Conclusion {
                quiz: (!conclusion.quiz.is_empty()).then(|| QuizProgress {
                    items: conclusion.quiz.len(),
                    answers: exploration.quiz_answers(&turn.update.request),
                }),
            },
            None => LatestTurn::Question,
        }
    }

    /// Whether the agent's latest turn posted a conclusion.
    pub(super) fn concluded(&self) -> bool {
        matches!(self.latest, LatestTurn::Conclusion { .. })
    }

    /// The conclusion's quiz, once the round concluded with one.
    pub(super) fn quiz(&self) -> Option<QuizProgress<'a>> {
        match self.latest {
            LatestTurn::Conclusion { quiz } => quiz,
            _ => None,
        }
    }

    /// Whether the round went past the question step `step`.
    pub(super) fn passed(&self, step: &QuestionStep<'_>) -> bool {
        self.current != Current::Question(step.number)
    }
}
