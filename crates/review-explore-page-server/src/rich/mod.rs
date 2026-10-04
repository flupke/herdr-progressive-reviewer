//! The rich data set, which the screenshot gallery shows: a round as long and as rich as a real
//! one, about a synthetic change that batches the reviewer's replies to the agent. Its shapes
//! follow real rounds: a design in four full parts, questions with several paragraphs of Context,
//! tables, diagrams, three citations and long choices, and a long conclusion.

mod change;
mod conclusion;
mod design;
mod questions;

use review_explore::{Conclusion, Design, Question};
use review_explore_page::{QuestionMarks, ReviewName, TurnResponse};

use crate::changed_source::FixedChange;
use crate::round_data::RoundData;

pub(crate) struct Rich;

impl RoundData for Rich {
    fn review(&self) -> ReviewName {
        ReviewName {
            repository: "review-notes".into(),
            revision: "wqzlpnrt".into(),
            title: "Batch the reviewer's replies into one agent notification".into(),
        }
    }

    fn design(&self) -> Design {
        design::design()
    }

    fn question(&self, number: usize) -> (Question, QuestionMarks) {
        questions::question(number)
    }

    fn answer_response(&self, number: usize) -> TurnResponse {
        questions::answer_response(number)
    }

    fn conclusion(&self, quiz: bool) -> Conclusion {
        conclusion::conclusion(quiz)
    }

    fn conclusion_response(&self) -> TurnResponse {
        conclusion::conclusion_response()
    }

    fn change(&self) -> &FixedChange {
        &change::CHANGE
    }
}

#[cfg(test)]
mod tests {
    use review_explore_page::RoundStage;

    use super::*;

    /// Every citation of the rich questions shows lines of the change, except the one of a file
    /// outside it: a diff that falls out of step with its files would show none.
    #[test]
    fn rich_citations_show_lines() {
        for number in 1..=3 {
            let RoundStage::Question { citations, .. } = Rich.question_stage(number, None) else {
                panic!("question {number} is not a question");
            };
            let shown: Vec<bool> = citations
                .iter()
                .map(|citation| citation.lines.is_ok())
                .collect();
            let expected = if number == 3 {
                vec![true, false]
            } else {
                vec![true; citations.len()]
            };
            assert_eq!(shown, expected, "question {number}");
        }
        for item in Rich.quiz_items() {
            for proof in &item.proof {
                assert!(Rich.change().cite(proof).lines.is_ok(), "{}", item.question);
            }
        }
    }
}
