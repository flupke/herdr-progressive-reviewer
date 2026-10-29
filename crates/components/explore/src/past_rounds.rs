//! The button that opens the Explore page on its list of past rounds, while no round runs: on
//! the start screen, active even when nothing is left to review, and after a start that failed
//! or was cancelled. It starts no round.

use review_explore_page_opening::PageScreen;
use ui_actions::Action;
use ui_controls::KeyHint;
use ui_shortcuts::ExploreTurnShortcut;

use super::start::key_hint;
use super::{Control, ExploreComponent, Progress, flow::ConversationLayout};

impl ExploreComponent {
    /// Whether no round runs or starts, so that the pane offers the past rounds.
    fn offers_past_rounds(&self) -> bool {
        self.exploration.is_none() && matches!(self.progress, Progress::Ready | Progress::Retryable)
    }

    /// Open the Explore page on its list of past rounds, while no round runs.
    pub(super) fn open_past_rounds(&mut self) -> Vec<Action> {
        if !self.offers_past_rounds() {
            return Vec::new();
        }
        self.open_page(PageScreen::PastRounds)
    }

    /// The button, under the controls of the start screen, while no round runs.
    pub(super) fn lay_out_past_rounds(&self, layout: &mut ConversationLayout) {
        if !self.offers_past_rounds() {
            return;
        }
        layout.gap();
        layout.controls([(
            "Past rounds on the Explore page".into(),
            Control::PastRounds,
        )]);
    }

    /// The footer's hint for the button, while no round runs.
    pub(super) fn past_rounds_hint(&self) -> Option<KeyHint> {
        self.offers_past_rounds()
            .then(|| key_hint(ExploreTurnShortcut::PastRounds, "past rounds"))
            .flatten()
    }
}
