//! A concluded round's quiz, and how far the reviewer is in it.

use serde::Serialize;
use ts_rs::TS;

use crate::QuizAnswers;

/// How far the reviewer is in the quiz.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuizStage {
    /// The round has not concluded yet.
    Later,
    /// The reviewer answers the item `item`, from 1, of `items` ("Quiz 2/3"). Right after an
    /// item is checked, the next one is named.
    Running { item: usize, items: usize },
    /// The reviewer answered or skipped every item ("Quiz 1/3"): `correct` of the `answered`
    /// items had the correct option; the reviewer skipped the others.
    Scored {
        correct: usize,
        answered: usize,
        items: usize,
    },
}

/// A conclusion's quiz, and what the reviewer answered of it.
#[derive(Clone, Copy, Debug)]
pub struct QuizProgress<'a> {
    /// The quiz's number of items.
    pub items: usize,
    /// What the reviewer answered; `None` before the first answer.
    pub answers: Option<&'a QuizAnswers>,
}

impl QuizProgress<'_> {
    /// Whether the reviewer has items left to answer.
    pub(super) fn running(&self) -> bool {
        self.next_item().is_some()
    }

    /// The item answered next, zero-based.
    fn next_item(&self) -> Option<usize> {
        match self.answers {
            Some(answers) => answers.next_item(self.items),
            None => Some(0),
        }
    }

    /// How far the reviewer is in the quiz.
    pub(super) fn stage(&self) -> QuizStage {
        let items = self.items;
        let finished = self
            .answers
            .filter(|answers| answers.next_item(items).is_none());
        match finished {
            Some(answers) => QuizStage::Scored {
                correct: answers.correct_picks(),
                answered: answers.picks.len(),
                items,
            },
            None => QuizStage::Running {
                item: self.next_item().unwrap_or(0) + 1,
                items,
            },
        }
    }
}
