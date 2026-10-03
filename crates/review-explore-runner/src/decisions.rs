//! What a kickoff says about the questions the reviewer decided in the
//! review's earlier rounds, so a fresh reader does not ask them again.

use crate::input::{Outcome, Quoted};
use review_explore::{Exploration, Interpretation, ReviewerAnswer, TopicStatus};
use std::fmt;

/// The questions the reviewer decided in a review's earlier rounds, oldest
/// first. A cancelled answer has left its round, so it is never here.
#[derive(Debug, Default)]
pub struct EarlierDecisions {
    decisions: Vec<Decision>,
}

/// An answer to a question that chose, and how the agent interpreted it.
#[derive(Debug)]
struct Decision {
    answer: ReviewerAnswer,
    /// None when the round ended before the agent's turn after the answer.
    interpretation: Option<Interpretation>,
}

impl EarlierDecisions {
    /// The decisions of `rounds`, given oldest first (see [`Decision::of`]).
    pub fn new<'a>(rounds: impl IntoIterator<Item = &'a Exploration>) -> Self {
        Self {
            decisions: rounds
                .into_iter()
                .flat_map(|round| {
                    round
                        .answers
                        .iter()
                        .filter_map(|answer| Decision::of(round, answer))
                })
                .collect(),
        }
    }
}

impl Decision {
    /// `answer` as a decision, when it decides its question: the agent
    /// interpreted it as accepting the choice or asking for changes, or the
    /// round ended before the agent's turn and it chose an option that does
    /// either. Context, questions to the agent and replies to a conclusion
    /// decide nothing.
    fn of(round: &Exploration, answer: &ReviewerAnswer) -> Option<Self> {
        answer.question.as_ref()?;
        let interpretation = round.interpretation(&answer.id);
        let decides = match interpretation {
            Some(interpretation) => interpretation.status != TopicStatus::Open,
            None if round.took_up(&answer.id) => false,
            None => answer
                .option
                .as_ref()
                .is_some_and(|option| option.outcome != TopicStatus::Open),
        };
        decides.then(|| Self {
            answer: answer.clone(),
            interpretation: interpretation.cloned(),
        })
    }
}

impl fmt::Display for EarlierDecisions {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.decisions.is_empty() {
            return Ok(());
        }
        writeln!(
            output,
            "\nEarlier decisions of this review, oldest first (text quoted):"
        )?;
        for decision in &self.decisions {
            write!(output, "{decision}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Decision {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        let answer = &self.answer;
        let Some(question) = &answer.question else {
            return Ok(());
        };
        write!(
            output,
            "\nDecided answer: {} (question {} version {})\n{}",
            answer.id,
            question.id,
            question.version,
            Quoted(&question.text)
        )?;
        match &answer.option {
            Some(option) => write!(output, "Choice: {}\n{}", option.id, Quoted(&option.text))?,
            None => writeln!(output, "Choice: none")?,
        }
        if !answer.text.is_empty() {
            write!(output, "Comment:\n{}", Quoted(&answer.text))?;
        }
        match (&self.interpretation, &answer.option) {
            (Some(interpretation), _) => {
                writeln!(output, "Outcome: {}", Outcome(interpretation.status))?;
                for follow_up in &interpretation.follow_ups {
                    write!(output, "Follow-up:\n{}", Quoted(follow_up))?;
                }
            }
            (None, Some(option)) => writeln!(
                output,
                "Outcome: {} (the option's; never interpreted)",
                Outcome(option.outcome)
            )?,
            (None, None) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "decisions.tests.rs"]
mod tests;
