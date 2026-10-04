//! The agent's conclusion, as the page shows it: its summary with the reviewer's decisions, the
//! list to be implemented with what became of its latest implementation request, the future
//! work, and its quiz.

use review_explore::{Conclusion, Decision, DecisionTag, QuizItem};
use review_explore_citations::Citation;
use serde::Serialize;
use ts_rs::TS;

use super::question::CitationView;
use super::{markdown, markdown_if_any};
use crate::round::{
    ImplementationState, PageImplementation, PageQuiz, RoundSnapshot, RoundStage, list_items,
};
use crate::status::StatusCard;

#[derive(Debug, Serialize, TS)]
pub(crate) struct ConclusionView {
    /// The request of the agent's turn that posted the conclusion.
    request: String,
    /// The summary's first paragraph, which leads the conclusion; `None` when the summary
    /// opens with something else.
    lead_html: Option<String>,
    /// The rest of the summary, under the reviewer's decisions: its limitations and remaining
    /// uncertainty.
    summary_html: String,
    /// The reviewer's decisions in the round, in the order of the round rail.
    decisions: Vec<DecisionView>,
    future_work_html: Option<String>,
    /// The list to be implemented that the form starts from, as raw text: the agent's, or the
    /// reviewer's own list of a request that was not sent.
    draft: String,
    /// How the panel shows the list, with the actions around it.
    list: ListView,
    /// The latest implementation request of the conclusion, with its list rendered.
    implementation: Option<ImplementationView>,
    /// What became of it, as a status card with the action that recovers it.
    implementation_card: Option<StatusCard>,
    /// The words that open the reply to the conclusion; `None` when the page offers no actions
    /// on the conclusion, as in an earlier round.
    reply_label: Option<&'static str>,
    /// The conclusion's quiz, when it has one.
    quiz: Option<QuizView>,
}

/// How the panel shows the list to be implemented, and the actions around it besides the card's.
#[derive(Debug, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ListView {
    /// The draft, which the reviewer edits, then sends with Implement.
    Editable,
    /// The draft, read only: the page offers no action on it.
    Draft { html: String, items: usize },
    /// The latest request's list, read only.
    Request {
        /// What the list is, before its count: "Saved request", "The request", "Sent".
        label: &'static str,
        /// Whether the request is out of the reviewer's hands, which mutes its list.
        settled: bool,
        /// Whether "Edit before sending" offers to edit the draft and send it in place of the
        /// request.
        edit: bool,
        /// Whether "Start a new round…" offers Reset.
        new_round: bool,
    },
}

/// A question the reviewer answered, and the answer kept: a line of "Your decisions".
#[derive(Debug, Serialize, TS)]
struct DecisionView {
    /// The question's number in the round rail.
    number: usize,
    /// The question's text, the agent's Markdown rendered.
    question_html: String,
    /// The text of the kept choice; `None` for a comment-only answer.
    choice: Option<String>,
    /// The reviewer's comment; empty when there is none.
    comment: String,
    /// How the kept choice relates to the reviewer's first pick and to the agent's
    /// recommendation.
    #[ts(type = "Array<\"changed_after_first_pick\" | \"as_recommended\">")]
    tags: Vec<DecisionTag>,
}

/// An implementation request of the conclusion.
#[derive(Debug, Serialize, TS)]
struct ImplementationView {
    /// The request's delivery identity.
    delivery: String,
    text_html: String,
    /// How many items its list has: its lines that are not blank.
    items: usize,
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
        let decisions = round
            .overview
            .as_deref()
            .map_or(&[][..], |overview| &overview.decisions[..]);
        Some(Self::new(
            &round.stage,
            request,
            conclusion,
            implementation.as_ref(),
            quiz,
            decisions,
            offers_actions,
        ))
    }

    fn new(
        stage: &RoundStage,
        request: &str,
        conclusion: &Conclusion,
        implementation: Option<&PageImplementation>,
        quiz: &PageQuiz,
        decisions: &[Decision],
        offers_actions: bool,
    ) -> Self {
        let replaces = implementation.map(|implementation| implementation.delivery.as_str());
        let offers_implement = offers_actions && stage.offers_implement(request, replaces);
        let draft = match implementation {
            Some(implementation) if offers_implement => &implementation.text,
            _ => &conclusion.to_be_implemented,
        };
        let (lead_html, summary_html) = lead(markdown(&conclusion.summary, 2));
        Self {
            request: request.to_owned(),
            lead_html,
            summary_html,
            decisions: decisions.iter().map(DecisionView::new).collect(),
            future_work_html: markdown_if_any(&conclusion.future_work, 3),
            draft: draft.clone(),
            list: ListView::new(draft, implementation, offers_actions, offers_implement),
            implementation: implementation.map(|implementation| ImplementationView {
                delivery: implementation.delivery.clone(),
                text_html: markdown(&implementation.text, 3),
                items: implementation.items(),
            }),
            implementation_card: implementation.map(|implementation| {
                StatusCard::implementation(
                    implementation,
                    request,
                    offers_actions,
                    offers_implement,
                )
            }),
            reply_label: offers_actions.then_some(match implementation.map(|i| &i.state) {
                Some(ImplementationState::Sent) => "Reply to the agent",
                _ => "Not ready? Reply to the agent instead",
            }),
            quiz: (!conclusion.quiz.is_empty()).then(|| QuizView::new(&conclusion.quiz, quiz)),
        }
    }
}

impl ListView {
    /// How the panel shows `draft`, or the list of the latest request `implementation`.
    /// `offers_actions` tells whether the page offers actions on the conclusion,
    /// `offers_implement` whether it offers a new request.
    fn new(
        draft: &str,
        implementation: Option<&PageImplementation>,
        offers_actions: bool,
        offers_implement: bool,
    ) -> Self {
        use ImplementationState::{Cancelled, NotSent, NotStarted, Paused, Sending, Sent, Unknown};
        let request = |label, settled| Self::Request {
            label,
            settled,
            edit: false,
            new_round: false,
        };
        let Some(implementation) = implementation else {
            return if offers_implement {
                Self::Editable
            } else {
                Self::Draft {
                    html: markdown(draft, 3),
                    items: list_items(draft),
                }
            };
        };
        if !offers_actions {
            return request("The request", true);
        }
        match implementation.state {
            Paused => Self::Request {
                label: "Saved request",
                settled: false,
                edit: offers_implement,
                new_round: false,
            },
            NotStarted => request("The request", false),
            Sent => Self::Request {
                label: "Sent",
                settled: true,
                edit: false,
                new_round: true,
            },
            NotSent(_) | Cancelled if offers_implement => Self::Editable,
            Unknown | Sending | NotSent(_) | Cancelled => request("The request", true),
        }
    }
}

/// The rendered summary `html` split into its first paragraph, which leads the conclusion, and
/// the rest; no lead when it opens with something else than a paragraph.
fn lead(html: String) -> (Option<String>, String) {
    const END: &str = "</p>";
    match html.find(END) {
        Some(end) if html.starts_with("<p>") => {
            let (lead, rest) = html.split_at(end + END.len());
            (Some(lead.to_owned()), rest.trim_start().to_owned())
        }
        _ => (None, html),
    }
}

impl DecisionView {
    fn new(decision: &Decision) -> Self {
        Self {
            number: decision.number,
            question_html: markdown(&decision.question, 3),
            choice: decision.answer.choice.clone(),
            comment: decision.answer.comment.clone(),
            tags: decision.answer.tags.clone(),
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
