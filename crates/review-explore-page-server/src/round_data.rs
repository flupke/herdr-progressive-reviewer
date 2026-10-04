//! What the standalone server's agent posts, and the change it cites. Two data sets exist:
//! [`Short`], small enough for the e2e tests to name what they check, and [`Rich`], as long and
//! as rich as a real round, for the screenshot gallery (`make explore-gallery`). The server's
//! `--data` option picks one.

use review_explore::{Conclusion, Design, Question, QuizAnswers, QuizItem};
use review_explore_page::{
    PageImplementation, PageQuiz, QuestionMarks, ReviewName, RoundStage, TurnResponse,
};

use review_explore::{CodeLocation, NotRelevantMark};
use review_explore_tally::{Gain, PendingLines, Share};

use crate::changed_source::FixedChange;
use crate::rich::Rich;
use crate::short::Short;

/// The request of the agent's turn that posts the fixed conclusion.
const CONCLUSION_REQUEST: &str = "conclusion";

/// A data set: the review, the design, the questions and the conclusion of a round, and the
/// change their citations name.
pub(crate) trait RoundData: Send + Sync {
    /// The review the start screen names.
    fn review(&self) -> ReviewName;

    /// The design of the change, which the round explains on its first turn.
    fn design(&self) -> Design;

    /// The agent's fixed question `number`, from 1, and the lines an answer to it marks.
    fn question(&self, number: usize) -> (Question, QuestionMarks);

    /// What the agent says back to the reviewer's answer to the previous question before its
    /// question `number`, from 2.
    fn answer_response(&self, number: usize) -> TurnResponse;

    /// The comment of the reviewer's answers in the pane; empty for none.
    fn pane_comment(&self) -> &'static str {
        ""
    }

    /// The fixed conclusion: with its quiz when `quiz`, or else with the reason it has none.
    fn conclusion(&self, quiz: bool) -> Conclusion;

    /// What the agent says back to the reviewer's last answer above its conclusion.
    fn conclusion_response(&self) -> TurnResponse;

    /// The change the citations name.
    fn change(&self) -> &FixedChange;

    /// The round's question `number`, from 1: `question` when given, which marks nothing, or
    /// else the fixed questions in turn. A question after the first follows the agent's fixed
    /// response to the reviewer's previous answer.
    fn question_stage(&self, number: usize, question: Option<Question>) -> RoundStage {
        let (question, marks) = match question {
            Some(question) => (question, QuestionMarks::default()),
            None => self.question(number),
        };
        let change = self.change();
        let citations = question
            .evidence
            .iter()
            .map(|evidence| change.cite(evidence))
            .collect();
        RoundStage::Question {
            number,
            question: Box::new(question),
            citations,
            marks,
            response: if number > 1 {
                self.answer_response(number)
            } else {
                TurnResponse::default()
            },
            answer_cancelled: false,
        }
    }

    /// The fixed conclusion, with the latest implementation request the reviewer authorized,
    /// and its quiz with the reviewer's answers when `quiz` is given.
    fn conclusion_stage(
        &self,
        implementation: Option<PageImplementation>,
        quiz: Option<QuizAnswers>,
    ) -> RoundStage {
        let conclusion = self.conclusion(quiz.is_some());
        let change = self.change();
        let quiz = quiz
            .map(|answers| PageQuiz {
                proofs: conclusion
                    .quiz
                    .iter()
                    .map(|item| item.proof.iter().map(|proof| change.cite(proof)).collect())
                    .collect(),
                answers,
                takes_answers: true,
            })
            .unwrap_or_default();
        RoundStage::Conclusion {
            request: CONCLUSION_REQUEST.into(),
            conclusion: Box::new(conclusion),
            implementation,
            quiz,
            response: self.conclusion_response(),
        }
    }

    /// The items of the conclusion's quiz.
    fn quiz_items(&self) -> Vec<QuizItem> {
        self.conclusion(true).quiz
    }

    /// The conclusion's list to be implemented.
    fn to_be_implemented(&self) -> String {
        self.conclusion(false).to_be_implemented
    }
}

/// What answering a question whose answer marks `marks` adds to the reviewed share of the
/// change: the change has 135 changed lines, 52 of them marked before the question; `None` when
/// the answer marks nothing.
pub(crate) fn fixed_gain(marks: &QuestionMarks) -> Option<Gain> {
    const CHANGED: u64 = 135;
    const MARKED: u64 = 52;
    let pending = PendingLines {
        reviewed: line_count(&marks.reviewed),
        not_relevant: line_count(NotRelevantMark::locations(&marks.not_relevant)),
    };
    let reopened = line_count(&marks.reopened);
    if pending.reviewed + pending.not_relevant + reopened == 0 {
        return None;
    }
    Some(Gain::new(pending, reopened, Share::of(MARKED, CHANGED)))
}

/// How many lines `locations` name; a whole file counts none.
fn line_count<'a>(locations: impl IntoIterator<Item = &'a CodeLocation>) -> u64 {
    locations
        .into_iter()
        .filter_map(|location| location.lines.as_ref())
        .map(|lines| u64::from(lines.count()))
        .sum()
}

/// The data set named `name`: `short` or `rich`.
pub(crate) fn by_name(name: &str) -> Option<&'static dyn RoundData> {
    match name {
        "short" => Some(&Short),
        "rich" => Some(&Rich),
        _ => None,
    }
}
