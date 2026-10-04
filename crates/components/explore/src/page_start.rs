//! A round the reviewer started on the Explore page: the pane shows it starting, can stop it,
//! and shows the round once the session announces its saved kickoff, as if the pane had
//! started it. A kickoff the session refuses, from the page or the pane, leaves the pane on its
//! start screen with the reason.
use super::{ExploreComponent, Progress};
use review_explore::{Command, RoundFront};
use ui_actions::Action;
use ui_events::{ExplorePageStart, ExplorePosted};

impl ExploreComponent {
    pub(super) fn page_start(&mut self, event: &ExplorePageStart) {
        if !self.on_start_screen() {
            return;
        }
        match &event.0 {
            Ok(()) => {
                // A round started on the page is followed there.
                self.front = RoundFront::Page;
                self.progress = Progress::Waiting;
                self.status = "Starting the round started on the Explore page…".into();
            }
            Err(reason) => self.page_start_failed(reason),
        }
    }

    /// Whether the pane waits for a round started on the Explore page.
    pub(super) fn awaiting_page_start(&self) -> bool {
        self.exploration.is_none() && self.progress == Progress::Waiting
    }

    /// Stop waiting for the round started on the Explore page: the session drops its kickoff.
    pub(super) fn stop_page_start(&mut self) -> Vec<Action> {
        self.show_start_screen();
        vec![Action::Explore(Command::Cancel)]
    }

    /// Show the round of a kickoff that the Explore page posted while the pane waits for the
    /// round it started: the agent works on its kickoff. Returns whether `event` is such a
    /// kickoff. A kickoff that arrives after Stop waiting belongs to the round the session
    /// cancels next, and the pane stays on its start screen.
    pub(super) fn adopt_started(&mut self, event: &ExplorePosted) -> bool {
        if !self.awaiting_page_start() || !event.request.is_kickoff() {
            return false;
        }
        match &event.result {
            Ok(round) => {
                self.challenger = round.exploration.challenger;
                self.open_round(round.exploration.clone());
                self.adopt(round);
                self.status = "Waiting for the implementation agent…".into();
                self.progress = Progress::Waiting;
            }
            Err(reason) => self.page_start_failed(reason),
        }
        true
    }

    /// Show the start screen again when the session refused the kickoff the pane posted: the
    /// round was not saved. Returns whether `event` is such a refusal.
    pub(super) fn refused_kickoff(&mut self, event: &ExplorePosted) -> bool {
        let Err(reason) = &event.result else {
            return false;
        };
        let posted = self
            .durable
            .posting
            .as_ref()
            .is_some_and(|request| request.request == event.request.request);
        if !posted || !event.request.is_kickoff() {
            return false;
        }
        self.show_start_screen();
        self.status = self.failed_start("The round could not start", reason);
        true
    }

    /// What the pane says of a start that failed for `reason`, after `failure`. A start that
    /// found nothing left to review leaves the reason to the line under the inactive buttons.
    pub(super) fn failed_start(&self, failure: &str, reason: &str) -> String {
        if self
            .start_block
            .is_some_and(|block| block.reason() == reason)
        {
            format!("{failure}.")
        } else {
            format!("{failure}: {reason}")
        }
    }

    /// The pane shows no round and does not start one of its own.
    fn on_start_screen(&self) -> bool {
        self.exploration.is_none() && !self.progress.awaiting_capture()
    }

    fn page_start_failed(&mut self, reason: &str) {
        if self.awaiting_page_start() {
            self.progress = Progress::Ready;
            self.status = self.failed_start(
                "The round started on the Explore page could not start",
                reason,
            );
        }
    }
}
