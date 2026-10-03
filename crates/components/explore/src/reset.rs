//! Reset closes the round and returns to the start screen, after a confirming click.
use super::{Control, ExploreComponent, navigation::Labels};
use review_explore::Command;
use std::time::{Duration, Instant};
use ui_actions::Action;
use ui_events::{ReviewNavigation, ReviewPane, ReviewPaneFocusRequested, ToastExpirationTick};

/// How long a first Reset click waits for the confirming one.
const CONFIRMATION: Duration = Duration::from_secs(5);

/// A first Reset click, waiting for the confirming one until its deadline.
#[derive(Default)]
pub(super) struct ResetConfirmation {
    deadline: Option<Instant>,
}

impl ResetConfirmation {
    /// Whether this click confirms a waiting one; otherwise it starts waiting.
    fn confirms(&mut self, now: Instant) -> bool {
        if self.deadline.take().is_some_and(|deadline| now < deadline) {
            return true;
        }
        self.deadline = Some(now + CONFIRMATION);
        false
    }

    pub(super) fn cancel(&mut self) {
        self.deadline = None;
    }

    fn is_waiting(&self) -> bool {
        self.deadline.is_some()
    }

    fn lapses_by(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| deadline <= now)
    }
}

impl ExploreComponent {
    /// Ask for confirmation, or with it close the round and show the start screen.
    pub(super) fn reset(&mut self, now: Instant) -> Vec<Action> {
        if !self.can_reset() || !self.reset.confirms(now) {
            return Vec::new();
        }
        self.show_start_screen();
        if self.mode == ReviewNavigation::Explore {
            self.events
                .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
        }
        vec![Action::Explore(Command::Reset)]
    }

    /// Show the start screen, keeping the navigation mode, storage and the page on the network.
    pub(super) fn show_start_screen(&mut self) {
        let mode = self.mode;
        let enabled = self.durable.enabled;
        let network_page = self.network_page.take();
        *self = Self::with_keymap(self.events.clone(), self.keymap.clone());
        self.mode = mode;
        self.durable.enabled = enabled;
        self.network_page = network_page;
    }

    fn can_reset(&self) -> bool {
        self.exploration.is_some() && self.durable.error.is_none()
    }

    /// The Reset control of the navigation bar's corner, or its confirmation.
    pub(super) fn reset_controls(&self) -> Labels {
        if !self.can_reset() {
            Vec::new()
        } else if self.reset.is_waiting() {
            vec![
                ("Reset closes this round for good.".into(), None),
                ("Confirm reset".into(), Some(Control::ConfirmReset)),
            ]
        } else {
            vec![("Reset".into(), Some(Control::Reset))]
        }
    }

    /// Whether the Reset confirmation lapses by `now`.
    pub fn reset_lapses_by(&self, now: Instant) -> bool {
        self.reset.lapses_by(now)
    }

    pub(super) fn expiration_tick(&mut self, event: &ToastExpirationTick) {
        if self.reset.lapses_by(event.now) {
            self.reset.cancel();
        }
    }
}
