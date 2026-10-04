//! The agent's conclusion, as the page shows it: its summary, the list to be implemented with
//! what became of its latest implementation request, the future work, and its quiz.

use review_explore::{Conclusion, QuizItem};
use review_explore_citations::Citation;
use serde::Serialize;
use ts_rs::TS;

use super::question::CitationView;
use super::{markdown, markdown_if_any};
use crate::round::{ImplementationState, PageImplementation, PageQuiz, RoundSnapshot, RoundStage};
use crate::status::StatusCard;

#[derive(Debug, Serialize, TS)]
pub(crate) struct ConclusionView {
    /// The request of the agent's turn that posted the conclusion.
    request: String,
    summary_html: String,
    future_work_html: Option<String>,
    /// The list to be implemented that the form starts from, as raw text: the agent's, or the
    /// reviewer's own list of a request that was not sent.
    draft: String,
    /// The same list rendered, shown when the page does not offer to edit it.
    draft_html: String,
    /// Whether the page offers Implement.
    offers_implement: bool,
    /// The latest implementation request of the conclusion, with its list rendered.
    implementation: Option<ImplementationView>,
    /// What became of it, as a status card with the action that recovers it.
    implementation_card: Option<StatusCard>,
    /// Whether the page offers actions on the conclusion: Implement and its recoveries, and
    /// Reply. An earlier round offers none.
    offers_actions: bool,
    /// The conclusion's quiz, when it has one.
    quiz: Option<QuizView>,
}

/// An implementation request of the conclusion.
#[derive(Debug, Serialize, TS)]
struct ImplementationView {
    /// The request's delivery identity.
    delivery: String,
    text_html: String,
    state: ImplementationState,
}

/// A conclusion's quiz: an item at a time before the conclusion, until the reviewer answered or
/// skipped every item, then the results beside the conclusion.
#[derive(Debug, Serialize, TS)]
struct QuizView {
    items: Vec<QuizItemView>,
    /// The item the reviewer answers next, from 0; `None` once the quiz is done, or when the
    /// round cannot save answers.
    next: Option<usize>,
    /// How many picks were correct, and how many items have a pick.
    correct_picks: usize,
    picked: usize,
    /// Whether the reviewer skipped the items that have no pick.
    skipped: bool,
}

/// One quiz item, and the reviewer's pick once there is one.
#[derive(Debug, Serialize, TS)]
struct QuizItemView {
    /// The item's position, from 1.
    number: usize,
    question: String,
    answers: Vec<QuizAnswerView>,
    /// Whether the reviewer picked the correct option, once the reviewer picked one.
    picked_correct: Option<bool>,
    /// The correct option's text.
    correct_answer: String,
    why: String,
    proof: Vec<CitationView>,
}

#[derive(Debug, Serialize, TS)]
struct QuizAnswerView {
    /// The option's position, from 0, as a pick sends it.
    index: usize,
    text: String,
    correct: bool,
    picked: bool,
}

impl ConclusionView {
    /// The conclusion `round` shows; `offers_actions` tells whether the page offers actions on
    /// it.
    pub(crate) fn of(round: &RoundSnapshot, offers_actions: bool) -> Option<Self> {
        let RoundStage::Conclusion {
            request,
            conclusion,
            implementation,
            quiz,
            ..
        } = &round.stage
        else {
            return None;
        };
        Some(Self::new(
            &round.stage,
            request,
            conclusion,
            implementation.as_ref(),
            quiz,
            offers_actions,
        ))
    }

    fn new(
        stage: &RoundStage,
        request: &str,
        conclusion: &Conclusion,
        implementation: Option<&PageImplementation>,
        quiz: &PageQuiz,
        offers_actions: bool,
    ) -> Self {
        let replaces = implementation.map(|implementation| implementation.delivery.as_str());
        let offers_implement = offers_actions && stage.offers_implement(request, replaces);
        let draft = match implementation {
            Some(implementation) if offers_implement => &implementation.text,
            _ => &conclusion.to_be_implemented,
        };
        Self {
            request: request.to_owned(),
            summary_html: markdown(&conclusion.summary, 2),
            future_work_html: markdown_if_any(&conclusion.future_work, 3),
            draft: draft.clone(),
            draft_html: markdown(draft, 3),
            offers_implement,
            implementation: implementation.map(|implementation| ImplementationView {
                delivery: implementation.delivery.clone(),
                text_html: markdown(&implementation.text, 3),
                state: implementation.state.clone(),
            }),
            implementation_card: implementation.map(|implementation| {
                StatusCard::implementation(
                    implementation,
                    request,
                    offers_actions,
                    offers_implement,
                )
            }),
            offers_actions,
            quiz: (!conclusion.quiz.is_empty()).then(|| QuizView::new(&conclusion.quiz, quiz)),
        }
    }
}

impl QuizView {
    fn new(items: &[QuizItem], quiz: &PageQuiz) -> Self {
        let answers = &quiz.answers;
        Self {
            items: items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let proof = quiz.proofs.get(index).map_or(&[][..], |proof| &proof[..]);
                    QuizItemView::new(index, item, proof, answers.pick(index).map(|p| p.answer))
                })
                .collect(),
            // A quiz the round cannot save answers to asks nothing: the conclusion shows.
            next: answers
                .next_item(items.len())
                .filter(|_| quiz.takes_answers),
            correct_picks: answers.correct_picks(),
            picked: answers.picks.len(),
            skipped: answers.skipped,
        }
    }
}

impl QuizItemView {
    fn new(index: usize, item: &QuizItem, proof: &[Citation], picked: Option<usize>) -> Self {
        Self {
            number: index + 1,
            question: item.question.clone(),
            answers: item
                .answers
                .iter()
                .enumerate()
                .map(|(option, text)| QuizAnswerView {
                    index: option,
                    text: text.clone(),
                    correct: option == item.correct,
                    picked: picked == Some(option),
                })
                .collect(),
            picked_correct: picked.map(|picked| picked == item.correct),
            correct_answer: item.answers.get(item.correct).cloned().unwrap_or_default(),
            why: item.why.clone(),
            proof: proof.iter().map(CitationView::new).collect(),
        }
    }
}
