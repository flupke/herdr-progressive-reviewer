//! The reviewer's answer that the agent's turn carries, as the page shows it while the agent
//! works on the turn or the turn waits for Retry (docs/design/explore-page/README.md,
//! "3. Waiting"): the turn's status card, then the question the answer answers, read only; and
//! in the panel, the answer that was sent, what it marked, and the card's action.

use review_explore::{KeptAnswer, MarkPhrase, MarkTense};
use serde::Serialize;
use ts_rs::TS;

use super::QuestionView;
use crate::round::RoundSnapshot;
use crate::status::StatusCard;

#[derive(Debug, Serialize, TS)]
pub(crate) struct SentView {
    /// The status card of the turn, whose actions the panel holds.
    card: StatusCard,
    /// The number on the rail of the question the answer answers, when known.
    number: Option<usize>,
    /// The question the answer answers, as the reviewer answered it; `None` for a reply to the
    /// conclusion.
    question: Option<QuestionView>,
    /// The choice and the comment the reviewer sent, and how the choice relates to the first
    /// pick and to the agent's recommendation.
    answer: KeptAnswer,
    /// What the answer marked: "Marked", then "12 lines reviewed", "3 lines not relevant";
    /// `None` when it marked nothing.
    marked: Option<MarkPhrase>,
}

impl SentView {
    /// The answer the agent's turn of `round` carries, if any, while the page offers actions on
    /// the round (`offers_actions`): an earlier round shows only that it is one, and the turn's
    /// card above the stage (design review, finding 6).
    pub(crate) fn of(round: &RoundSnapshot, offers_actions: bool) -> Option<Self> {
        let sent = round.stage.sent().filter(|_| offers_actions)?;
        let number = round.sent_number();
        let marked = sent.marked.phrase(MarkTense::Applied);
        Some(Self {
            card: StatusCard::of_turn(round, offers_actions)?,
            number,
            question: sent.question.as_ref().map(|answered| {
                QuestionView::answered(round.round.clone(), number.unwrap_or_default(), answered)
            }),
            answer: sent.kept.clone(),
            marked: (!marked.parts.is_empty()).then_some(marked),
        })
    }
}
