//! The start screen of a review with nothing left to review: its four buttons stay, inactive,
//! and a line says why, until a line is unreviewed again.

use std::sync::Arc;

use component_core::ComponentEventBus;
use ratatui::layout::Rect;
use review_explore::{Comparison, StartBlock};
use review_source::ReviewCheckpoint;
use ui_actions::Action;
use ui_events::{
    ExploreCaptured, ExplorePosted, ExploreProgress, ExploreRestored, ExploreStartBlock,
};

use super::rows;
use crate::flow::{Content, ConversationLayout};
use crate::start::StartButton;
use crate::{Control, ExploreComponent, Progress};

struct Pane {
    bus: ComponentEventBus<Action>,
    target: component_core::ComponentTarget,
}

impl Pane {
    /// The pane on its start screen.
    fn starting() -> Self {
        let mut bus = ComponentEventBus::<Action>::new();
        let target = bus.mount(|events| {
            ExploreComponent::with_keymap(events, comment_editor::KeymapSetting::default())
        });
        bus.publish(ExploreRestored {
            result: Ok(None),
            view: None,
            historical: false,
            storage_error: None,
            progress: ExploreProgress::Ready,
        })
        .unwrap();
        Self { bus, target }
    }

    fn component(&mut self) -> &mut ExploreComponent {
        self.bus.get_mut::<ExploreComponent>(self.target).unwrap()
    }

    fn block(&mut self, block: Option<StartBlock>) {
        self.bus.publish(ExploreStartBlock(block)).unwrap();
    }

    /// The start screen as the pane lays it out.
    fn layout(&mut self) -> ConversationLayout {
        let mut layout = ConversationLayout::new(Rect::new(0, 0, 120, 40));
        self.component()
            .status_controls(&mut layout, ui_theme::Theme::default().palette);
        layout
    }

    /// The labels of the start screen's buttons, the settings of the Explore page aside, and
    /// whether each one acts.
    fn buttons(&mut self) -> Vec<(String, bool)> {
        self.layout()
            .items
            .iter()
            .flat_map(|item| match &item.content {
                Content::Controls(buttons) => buttons
                    .iter()
                    .filter(|button| !matches!(button.control, Some(Control::Setting(_))))
                    .map(|button| (button.text.trim().to_owned(), button.control.is_some()))
                    .collect(),
                _ => Vec::new(),
            })
            .collect()
    }
}

fn labels(active: bool) -> Vec<(String, bool)> {
    StartButton::ALL
        .iter()
        .map(|button| (button.label.to_owned(), active))
        .collect()
}

#[test]
fn with_nothing_left_to_review_the_four_starts_are_inactive_and_say_why() {
    let mut pane = Pane::starting();

    pane.block(Some(StartBlock::NothingToReview));

    assert_eq!(pane.buttons(), labels(false));
    let layout = pane.layout();
    assert!(
        rows(&layout).contains(&StartBlock::NothingToReview.reason().to_owned()),
        "{:?}",
        rows(&layout)
    );
    for button in StartButton::ALL {
        assert!(
            pane.component().start(button.start).is_empty(),
            "{} starts nothing",
            button.label
        );
    }
    assert!(
        pane.component().footer_hints().is_empty(),
        "the footer shows no key that does nothing"
    );
}

#[test]
fn a_start_that_failed_offers_the_inactive_starts_in_place_of_retry_once_nothing_is_left() {
    let mut pane = Pane::starting();
    pane.component().start(StartButton::START_IN_PANE.start);
    pane.bus
        .publish(ExploreCaptured {
            result: Err("Repository comparison is not ready; retry Start".into()),
        })
        .unwrap();
    assert_eq!(pane.buttons(), [("Retry".to_owned(), true)]);

    pane.block(Some(StartBlock::NothingToReview));

    assert_eq!(pane.buttons(), labels(false));
    assert!(rows(&pane.layout()).contains(&StartBlock::NothingToReview.reason().to_owned()));
}

#[test]
fn once_a_line_is_unreviewed_the_starts_are_active_again() {
    let mut pane = Pane::starting();
    pane.block(Some(StartBlock::NothingToReview));

    pane.block(None);

    assert_eq!(pane.buttons(), labels(true));
    assert!(!rows(&pane.layout()).contains(&StartBlock::NothingToReview.reason().to_owned()));
    assert!(!pane.component().footer_hints().is_empty());
    assert!(
        !pane
            .component()
            .start(StartButton::START_IN_PANE.start)
            .is_empty()
    );
}

#[test]
fn a_kickoff_with_nothing_left_to_review_returns_the_pane_to_its_start_screen() {
    let mut pane = Pane::starting();
    pane.component().start(StartButton::START_IN_PANE.start);
    let comparison = Arc::new(Comparison {
        checkpoint: ReviewCheckpoint::new("review", "checkpoint"),
        repository_root: "/tmp".into(),
        files: vec![],
        diffs: vec![],
        context: vec![],
        manifest: vec![],
        sources: vec![],
        base: None,
    });
    let actions = pane
        .bus
        .publish(ExploreCaptured {
            result: Ok(comparison),
        })
        .unwrap()
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .collect::<Vec<_>>();
    let Some(Action::Explore(review_explore::Command::Turn(kickoff))) = actions.first() else {
        panic!("the pane sends its kickoff: {actions:?}");
    };
    let kickoff = (**kickoff).clone();
    assert_eq!(
        kickoff.instance,
        pane.component().exploration.as_ref().unwrap().instance
    );

    // Jev marked the rest before the kickoff went out.
    pane.block(Some(StartBlock::NothingToReview));
    pane.bus
        .publish(ExplorePosted {
            request: kickoff,
            result: Err(StartBlock::NothingToReview.reason().into()),
        })
        .unwrap();

    let component = pane.component();
    assert!(component.exploration.is_none());
    assert!(component.progress == Progress::Ready);
    assert!(
        component.status.contains("could not start"),
        "{}",
        component.status
    );
    assert_eq!(pane.buttons(), labels(false));
    assert!(
        rows(&pane.layout()).contains(&StartBlock::NothingToReview.reason().to_owned()),
        "the line under the start buttons says why"
    );
}
