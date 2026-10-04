//! A round on the Explore page: started with Start or Start with Challenger, or on the page. The
//! pane shows no question, answer or conclusion, only that the round runs on the page, its state
//! in one line, a button that opens the page again and one that shows the interview in the pane
//! instead. The page's address and QR code follow, as on every screen of the pane.

use review_explore::RoundFront;
use ui_actions::{Action, ExplorePageAction};
use ui_controls::KeyHint;
use ui_events::ExplorePaneStarts;
use ui_shortcuts::{ExploreCommand, ExploreShortcut};
use ui_theme::Palette;

use super::start::{FrontControl, PageRoundButton, StartButton};
use super::{Control, ExploreComponent, Progress, flow::ConversationLayout};

impl ExploreComponent {
    /// Whether the pane shows its round as a round on the page: one is shown or starting, and
    /// the reviewer follows it on the page.
    pub(super) fn shows_page_round(&self) -> bool {
        self.front == RoundFront::Page
            && (self.exploration.is_some()
                || self.progress.awaiting_capture()
                || self.awaiting_page_start())
    }

    pub(super) fn pane_starts_set(&mut self, event: &ExplorePaneStarts) {
        self.pane_starts = event.0;
    }

    /// Run a control that chooses where the reviewer follows the round: a start, or a button
    /// of a round on the page, which does nothing elsewhere.
    pub(super) fn front_control(&mut self, control: FrontControl) -> Vec<Action> {
        match control {
            FrontControl::Start(start) => self.start(start),
            FrontControl::OpenPage if self.shows_page_round() => self.open_page(),
            FrontControl::ContinueInPane if self.shows_page_round() => {
                self.continue_in_pane();
                Vec::new()
            }
            FrontControl::OpenPage | FrontControl::ContinueInPane => Vec::new(),
        }
    }

    /// A key while the pane shows a round on the page: only the keys of its buttons act, since
    /// the others would act on a question the pane hides.
    pub(super) fn page_round_key(&mut self, command: Option<ExploreCommand>) -> Vec<Action> {
        let Some(ExploreCommand::Explore(ExploreShortcut::Turn(shortcut))) = command else {
            return Vec::new();
        };
        PageRoundButton::of(shortcut)
            .map(|button| self.front_control(button.control))
            .unwrap_or_default()
    }

    /// The keys the footer shows for the pane: those of the four starts on the start screen,
    /// those of the buttons of a round on the page.
    pub fn footer_hints(&self) -> Vec<KeyHint> {
        if self.shows_page_round() {
            PageRoundButton::hints()
        } else if self.offers_starts() {
            StartButton::hints(self.start_block)
        } else {
            Vec::new()
        }
    }

    /// Open the page in the browser again; a failure says so again.
    fn open_page(&mut self) -> Vec<Action> {
        self.unopened_page = None;
        vec![Action::ExplorePage(ExplorePageAction::Open)]
    }

    /// Show the round's interview in the pane, at the question it would show there.
    fn continue_in_pane(&mut self) {
        self.front = RoundFront::Pane;
        // A round still starting opens nothing once its change is captured.
        self.open_page = false;
        if self.exploration.is_some() {
            self.reveal.set(Some(super::Reveal::Start));
            self.publish_evidence(self.view_id(), false);
        }
    }

    pub(super) fn lay_out_page_round(&self, layout: &mut ConversationLayout, palette: Palette) {
        layout.text(
            "This Explore round runs on the Explore page.",
            palette.text,
            None,
        );
        layout.text(self.page_round_state(), palette.dim, None);
        layout.gap();
        layout.controls(
            PageRoundButton::ALL
                .map(|button| (button.label.into(), Control::Front(button.control))),
        );
    }

    /// Where the round stands, in one line.
    fn page_round_state(&self) -> String {
        let Some(exploration) = &self.exploration else {
            return "Starting: the reviewer prepares the round.".into();
        };
        if self.durable.error.is_some() || self.durable.historical {
            return self.status.clone();
        }
        match self.progress {
            Progress::Retryable => format!("Interrupted: {}", self.status),
            Progress::Waiting | Progress::Capturing | Progress::DiscardingCapture => {
                "The agent is working.".into()
            }
            Progress::Ready if exploration.conclusion.is_some() => {
                "Concluded: the conclusion is on the page.".into()
            }
            Progress::Ready if exploration.questions.is_empty() => "The agent is working.".into(),
            Progress::Ready => "A question waits for your answer.".into(),
        }
    }
}
