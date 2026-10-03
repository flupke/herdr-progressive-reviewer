//! What a kickoff says about the questions the reviewer decided in the
//! review's earlier rounds, so a fresh reader does not ask them again.

use review_explore::{Exploration, ReviewerAnswer};
use std::fmt;

/// The reviewer's answers in a review's earlier rounds, oldest first, of
/// which a prompt lists those that decide a question. A cancelled answer has
/// left its round, so it is never here.
#[derive(Debug, Default)]
pub struct EarlierDecisions {
    answers: Vec<ReviewerAnswer>,
}

impl EarlierDecisions {
    /// The answers of `rounds`, given oldest first.
    pub fn new<'a>(rounds: impl IntoIterator<Item = &'a Exploration>) -> Self {
        Self {
            answers: rounds
                .into_iter()
                .flat_map(|round| &round.answers)
                .cloned()
                .collect(),
        }
    }
}

impl fmt::Display for EarlierDecisions {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A reply to a conclusion decides no question.
        let mut decided = self
            .answers
            .iter()
            .filter_map(|answer| Some((answer, answer.question.as_ref()?)))
            .peekable();
        if decided.peek().is_none() {
            return Ok(());
        }
        writeln!(
            output,
            "\nEarlier decisions of this review, oldest first (text quoted):"
        )?;
        for (answer, question) in decided {
            writeln!(
                output,
                "\nDecided answer: {} (question {} version {})",
                answer.id, question.id, question.version
            )?;
            crate::input::quote(output, &question.text)?;
            match &answer.option {
                Some(option) => {
                    writeln!(
                        output,
                        "Choice: {} ({})",
                        option.id,
                        crate::input::outcome(option.outcome)
                    )?;
                    crate::input::quote(output, &option.text)?;
                }
                None => writeln!(output, "Choice: none")?,
            }
            if !answer.text.is_empty() {
                writeln!(output, "Comment:")?;
                crate::input::quote(output, &answer.text)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "decisions.tests.rs"]
mod tests;
