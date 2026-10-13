//! The short data set, which the e2e tests check: a change of one file that keeps the reviewer's
//! unsent draft when a round reopens, two questions that take turns, and a short conclusion.

mod change;
mod design;
mod diagrams;
mod explanation;
mod question;
mod quiz;

use review_explore::{Conclusion, Design, Question};
use review_explore_page::{QuestionMarks, ReviewName, TurnResponse};
use review_repository::repository::ShortRevision;

use crate::changed_source::FixedChange;
use crate::round_data::RoundData;
use crate::tally::JevMark;

pub(crate) struct Short;

impl RoundData for Short {
    fn review(&self) -> ReviewName {
        ReviewName {
            repository: "drafts-demo".into(),
            revision: ShortRevision {
                prefix: "km".into(),
                rest: "zqvtyx".into(),
            },
            title: "Keep the reviewer's draft when a round reopens".into(),
        }
    }

    fn design(&self) -> Design {
        design::design()
    }

    fn glossary(&self) -> std::collections::BTreeMap<String, String> {
        [
            (
                "draft",
                "The text the reviewer typed in the answer box and has not sent yet.",
            ),
            (
                "reopen()",
                "The pane's call that rebuilds a round from what was saved when the pane opens \
                 again.",
            ),
        ]
        .map(|(term, definition)| (term.to_owned(), definition.to_owned()))
        .into()
    }

    fn question(&self, number: usize) -> (Question, QuestionMarks) {
        question::question(number)
    }

    fn answer_response(&self, _number: usize) -> TurnResponse {
        question::answer_response()
    }

    fn conclusion(&self, quiz: bool) -> Conclusion {
        question::conclusion(quiz)
    }

    fn conclusion_response(&self) -> TurnResponse {
        question::conclusion_response()
    }

    fn change(&self) -> &FixedChange {
        &change::CHANGE
    }

    fn jev_marks(&self) -> &'static [JevMark] {
        &[JevMark {
            path: change::DRAFTS.path,
            lines: 1,
        }]
    }
}
