//! The quiz a conclusion carries: a few questions at whiteboard level that check the reviewer
//! can explain how the system works after the change, and what the reviewer answered.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{Comparison, Conclusion, EvidenceRef, Exploration};

/// The most items a quiz holds.
const MOST_ITEMS: usize = 3;

/// One question of a conclusion's quiz, about a scenario in the running system.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuizItem {
    /// A short concrete scenario in the running system, with no code identifiers.
    pub question: String,
    /// Two to four options of similar length and form.
    #[schemars(length(min = 2, max = 4))]
    pub answers: Vec<String>,
    /// The zero-based index of the correct option in `answers`.
    pub correct: usize,
    /// One sentence that explains the correct option in design terms.
    pub why: String,
    /// The lines that establish the correct option, most decisive first, each with notes.
    pub proof: Vec<EvidenceRef>,
    /// Which whiteboard topic the item tests, and why it is not an implementation detail. The
    /// round keeps it; the reviewer does not see it.
    pub level: String,
}

/// What the reviewer does with a conclusion's quiz on the Explore page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuizResponse {
    /// Picks the option `answer` of the item `item`, both zero-based.
    Pick { item: usize, answer: usize },
    /// Skips the items not answered yet.
    Skip,
}

/// What the reviewer did with the quiz of one conclusion.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct QuizAnswers {
    /// The reviewer's pick for each item answered, in the order of the quiz.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub picks: Vec<QuizPick>,
    /// The reviewer skipped the items that have no pick.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
}

/// The reviewer's pick for one quiz item, and whether it was the correct option.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct QuizPick {
    /// The item, zero-based.
    pub item: usize,
    /// The option picked, zero-based.
    pub answer: usize,
    pub correct: bool,
}

impl QuizAnswers {
    /// Records `response` to `quiz`, the reviewer answering its items in order. Returns false
    /// when the answers already hold it: the same pick again, or a skip of nothing. Refuses a
    /// pick other than of the next item, a second pick that differs from the first, a pick
    /// after a skip, and an item or option the quiz does not have.
    pub fn record(&mut self, quiz: &[QuizItem], response: QuizResponse) -> eyre::Result<bool> {
        match response {
            QuizResponse::Skip => {
                let skips = !self.skipped && self.picks.len() < quiz.len();
                self.skipped |= skips;
                Ok(skips)
            }
            QuizResponse::Pick { item, answer } => {
                if let Some(earlier) = self.pick(item) {
                    eyre::ensure!(
                        earlier.answer == answer,
                        "quiz item {} has another answer already",
                        item + 1
                    );
                    return Ok(false);
                }
                eyre::ensure!(!self.skipped, "the reviewer skipped the quiz");
                eyre::ensure!(
                    item == self.picks.len(),
                    "quiz item {} is not the next to answer",
                    item + 1
                );
                let options = quiz
                    .get(item)
                    .ok_or_else(|| eyre::eyre!("the quiz has no item {}", item + 1))?;
                eyre::ensure!(
                    answer < options.answers.len(),
                    "quiz item {} has no option {}",
                    item + 1,
                    answer + 1
                );
                self.picks.push(QuizPick {
                    item,
                    answer,
                    correct: answer == options.correct,
                });
                Ok(true)
            }
        }
    }

    /// The reviewer's pick for the item `item`, zero-based.
    pub fn pick(&self, item: usize) -> Option<&QuizPick> {
        self.picks.iter().find(|pick| pick.item == item)
    }

    /// The item the reviewer answers next in a quiz of `items` items, zero-based; `None` once
    /// every item has a pick or the reviewer skipped the rest.
    pub fn next_item(&self, items: usize) -> Option<usize> {
        let next = self.picks.len();
        (!self.skipped && next < items).then_some(next)
    }

    /// How many picks were the correct option.
    pub fn correct_picks(&self) -> usize {
        self.picks.iter().filter(|pick| pick.correct).count()
    }
}

impl Conclusion {
    /// A quiz of at most three items, each with its correct option and its proof, or an empty
    /// quiz that says why it is empty.
    pub(crate) fn validate_quiz(&self, comparison: &Comparison) -> eyre::Result<()> {
        let reason = self
            .quiz_empty_reason
            .as_deref()
            .is_some_and(|reason| !reason.trim().is_empty());
        if self.quiz.is_empty() {
            eyre::ensure!(
                reason,
                "An empty quiz needs quiz_empty_reason: one sentence on why the change has \
                 nothing at whiteboard level"
            );
            return Ok(());
        }
        eyre::ensure!(
            !reason,
            "quiz_empty_reason explains an empty quiz; leave it null when the quiz has items"
        );
        eyre::ensure!(
            self.quiz.len() <= MOST_ITEMS,
            "A quiz has at most {MOST_ITEMS} items"
        );
        self.quiz.iter().enumerate().try_for_each(|(index, item)| {
            item.validate(comparison)
                .map_err(|error| eyre::eyre!("Quiz item {}: {error}", index + 1))
        })
    }
}

impl QuizItem {
    fn validate(&self, comparison: &Comparison) -> eyre::Result<()> {
        eyre::ensure!(
            [&self.question, &self.why, &self.level]
                .iter()
                .all(|text| !text.trim().is_empty()),
            "question, why and level need text"
        );
        let mut distinct = HashSet::new();
        eyre::ensure!(
            (2..=4).contains(&self.answers.len())
                && self.answers.iter().all(|answer| !answer.trim().is_empty()
                    && distinct.insert(answer.trim().to_lowercase())),
            "answers needs two to four distinct options"
        );
        eyre::ensure!(
            self.correct < self.answers.len(),
            "correct must be the zero-based index of one of its answers"
        );
        eyre::ensure!(
            !self.proof.is_empty()
                && self
                    .proof
                    .iter()
                    .all(|proof| proof.location.lines.is_some()
                        && comparison.validate_evidence(proof)),
            "proof needs the lines that establish the correct answer: valid paths and line \
             ranges, each with notes"
        );
        Ok(())
    }
}

impl Exploration {
    /// Records the reviewer's `response` to the quiz of the round's current conclusion, which
    /// the agent's turn `conclusion` posted. Returns false when the round already holds it.
    /// Refuses a response to another conclusion, or to a conclusion without a quiz.
    pub fn answer_quiz(&mut self, conclusion: &str, response: QuizResponse) -> eyre::Result<bool> {
        eyre::ensure!(
            self.conclusion_request() == Some(conclusion),
            "the quiz belongs to a conclusion that is no longer current"
        );
        let quiz = self
            .conclusion
            .as_ref()
            .map_or(&[][..], |current| &current.quiz);
        eyre::ensure!(!quiz.is_empty(), "the conclusion has no quiz");
        self.quiz_answers
            .entry(conclusion.to_owned())
            .or_default()
            .record(quiz, response)
    }

    /// What the reviewer did with the quiz of the conclusion the agent's turn `conclusion`
    /// posted.
    pub fn quiz_answers(&self, conclusion: &str) -> Option<&QuizAnswers> {
        self.quiz_answers.get(conclusion)
    }

    /// Drops the answers to the quizzes of conclusions the round no longer holds, after a turn
    /// was removed.
    pub(crate) fn forget_quiz_answers_of_withdrawn_conclusions(&mut self) {
        let conclusions: HashSet<&str> = self
            .conversation
            .iter()
            .filter(|turn| turn.update.conclusion.is_some())
            .map(|turn| turn.update.request.as_str())
            .collect();
        self.quiz_answers
            .retain(|conclusion, _| conclusions.contains(conclusion.as_str()));
    }
}
