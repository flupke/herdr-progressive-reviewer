//! How the reviewer chooses where to follow a round: the four buttons of the start screen, and
//! the two buttons of a round on the page, with their keys.

use review_explore::RoundFront;
use ui_controls::KeyHint;
use ui_shortcuts::{
    ExploreShortcut, ExploreStartShortcut, ExploreTurnShortcut, ShortcutSubscription,
};

/// How the reviewer starts a round from the start screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RoundStart {
    /// A challenger reviews beside the agent.
    pub(super) challenger: bool,
    /// Where the reviewer follows the round; on the page, it opens once the change is captured.
    pub(super) front: RoundFront,
}

/// One button of the start screen.
#[derive(Clone, Copy)]
pub(super) struct StartButton {
    pub(super) start: RoundStart,
    pub(super) shortcut: ExploreStartShortcut,
    pub(super) label: &'static str,
    /// What its key does, as the footer says it after the key.
    pub(super) hint: &'static str,
}

impl StartButton {
    pub(super) const START: Self = Self::new(
        false,
        RoundFront::Page,
        ExploreStartShortcut::Start,
        "Start",
        "start",
    );
    pub(super) const START_WITH_CHALLENGER: Self = Self::new(
        true,
        RoundFront::Page,
        ExploreStartShortcut::StartWithChallenger,
        "Start with Challenger",
        "with Challenger",
    );
    pub(super) const START_IN_PANE: Self = Self::new(
        false,
        RoundFront::Pane,
        ExploreStartShortcut::StartInPane,
        "Start in the pane",
        "in the pane",
    );
    pub(super) const START_IN_PANE_WITH_CHALLENGER: Self = Self::new(
        true,
        RoundFront::Pane,
        ExploreStartShortcut::StartInPaneWithChallenger,
        "Start in the pane with Challenger",
        "in the pane with Challenger",
    );

    /// The buttons in the order the start screen shows them.
    pub(super) const ALL: [Self; 4] = [
        Self::START,
        Self::START_WITH_CHALLENGER,
        Self::START_IN_PANE,
        Self::START_IN_PANE_WITH_CHALLENGER,
    ];

    const fn new(
        challenger: bool,
        front: RoundFront,
        shortcut: ExploreStartShortcut,
        label: &'static str,
        hint: &'static str,
    ) -> Self {
        Self {
            start: RoundStart { challenger, front },
            shortcut,
            label,
            hint,
        }
    }

    /// The button that `shortcut` presses.
    pub(super) fn of(shortcut: ExploreStartShortcut) -> Self {
        match shortcut {
            ExploreStartShortcut::Start => Self::START,
            ExploreStartShortcut::StartWithChallenger => Self::START_WITH_CHALLENGER,
            ExploreStartShortcut::StartInPane => Self::START_IN_PANE,
            ExploreStartShortcut::StartInPaneWithChallenger => Self::START_IN_PANE_WITH_CHALLENGER,
        }
    }
}

impl RoundStart {
    /// Whether this is the main start, the one button the start screen highlights.
    pub(super) fn is_main(self) -> bool {
        self == StartButton::START.start
    }
}

/// One button of a round on the page.
#[derive(Clone, Copy)]
pub(super) struct PageRoundButton {
    pub(super) label: &'static str,
    pub(super) control: FrontControl,
    pub(super) shortcut: ExploreTurnShortcut,
    /// What its key does, as the footer says it after the key.
    pub(super) hint: &'static str,
}

impl PageRoundButton {
    /// The buttons in the order the pane shows them.
    pub(super) const ALL: [Self; 2] = [
        Self {
            label: "Open the Explore page",
            control: FrontControl::OpenPage,
            shortcut: ExploreTurnShortcut::OpenPage,
            hint: "open the page",
        },
        Self {
            label: "Continue in the pane",
            control: FrontControl::ContinueInPane,
            shortcut: ExploreTurnShortcut::ContinueInPane,
            hint: "continue in the pane",
        },
    ];

    /// The button that `shortcut` presses, if it is one of them.
    pub(super) fn of(shortcut: ExploreTurnShortcut) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|button| button.shortcut == shortcut)
    }

    /// The footer's hints for the buttons.
    pub(super) fn hints() -> Vec<KeyHint> {
        Self::ALL
            .into_iter()
            .filter_map(|button| key_hint(button.shortcut, button.hint))
            .collect()
    }
}

impl StartButton {
    /// The footer's hints for the buttons.
    pub(super) fn hints() -> Vec<KeyHint> {
        Self::ALL
            .into_iter()
            .filter_map(|button| key_hint(ExploreTurnShortcut::Start(button.shortcut), button.hint))
            .collect()
    }
}

/// The key that runs `shortcut`, as the footer shows it with `hint`.
fn key_hint(shortcut: ExploreTurnShortcut, hint: &str) -> Option<KeyHint> {
    let key = ExploreShortcut::Turn(shortcut).keys().next()?;
    Some(KeyHint::new(key.label(), hint))
}

/// A control that chooses where the reviewer follows a round.
#[derive(Clone, Copy, Debug)]
pub(super) enum FrontControl {
    /// Start a round from the start screen.
    Start(RoundStart),
    /// Open the Explore page of a round on the page again.
    OpenPage,
    /// Show the interview of a round on the page in the pane.
    ContinueInPane,
}
