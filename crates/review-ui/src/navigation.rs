//! The one owner of the navigation mode, pane focus, open modals and layout.
//!
//! Components receive the mode through `ReviewNavigationChanged` and ask for
//! focus through `ReviewPaneFocusRequested`; neither event makes a component
//! the owner. [`NavigationRequests`] collects those requests on the event
//! bus, and the application applies them to [`Navigation`] after each
//! dispatch.

use component_core::{Component, ComponentSubscriptions};
use ui_events::{ReviewNavigation, ReviewNavigationChanged, ReviewPane, ReviewPaneFocusRequested};

use crate::Action;
use crate::layout::{ScreenLayout, Viewport};

/// The navigation state that decides what the reviewer sees and types into.
pub(crate) struct Navigation {
    mode: ReviewNavigation,
    focus: ReviewPane,
    remembered: FocusMemory,
    modals: OpenModals,
    layout: ScreenLayout,
}

/// The pane focus each mode had when the reviewer last left it.
#[derive(Clone, Copy)]
struct FocusMemory {
    files: ReviewPane,
    threads: ReviewPane,
    explore: ReviewPane,
}

impl FocusMemory {
    fn pane_mut(&mut self, mode: ReviewNavigation) -> &mut ReviewPane {
        match mode {
            ReviewNavigation::Files => &mut self.files,
            ReviewNavigation::Threads => &mut self.threads,
            ReviewNavigation::Explore => &mut self.explore,
        }
    }
}

/// A component shown in one of the two review panes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PaneComponent {
    Files,
    Threads,
    Explore,
    /// The diff: its own pane in Files and Threads, and the evidence shown
    /// inside Explore in Explore mode.
    Diff,
}

/// A component that takes all keyboard input while it is open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Modal {
    Revision,
    Overlay,
    Locations,
}

/// Which modals are open; the first in precedence order takes input.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct OpenModals {
    pub(crate) revision: bool,
    pub(crate) overlay: bool,
    pub(crate) locations: bool,
}

impl Modal {
    /// Open modals take input in this order.
    const PRECEDENCE: [Self; 3] = [Self::Revision, Self::Overlay, Self::Locations];
}

impl OpenModals {
    pub(crate) fn is_open(self, modal: Modal) -> bool {
        match modal {
            Modal::Revision => self.revision,
            Modal::Overlay => self.overlay,
            Modal::Locations => self.locations,
        }
    }

    fn top(self) -> Option<Modal> {
        Modal::PRECEDENCE
            .into_iter()
            .find(|modal| self.is_open(*modal))
    }
}

impl Navigation {
    pub(crate) fn new(viewport: Viewport) -> Self {
        let mode = ReviewNavigation::Files;
        let focus = ReviewPane::Navigation;
        Self {
            mode,
            focus,
            remembered: FocusMemory {
                files: focus,
                threads: focus,
                explore: focus,
            },
            modals: OpenModals::default(),
            layout: ScreenLayout::new(viewport, mode, focus, false),
        }
    }

    pub(crate) fn mode(&self) -> ReviewNavigation {
        self.mode
    }

    pub(crate) fn focus(&self) -> ReviewPane {
        self.focus
    }

    pub(crate) fn layout(&self) -> ScreenLayout {
        self.layout
    }

    /// Apply the mode, then the focus, that components requested.
    pub(crate) fn apply(&mut self, requests: NavigationRequests) {
        if let Some(mode) = requests.mode
            && mode != self.mode
        {
            *self.remembered.pane_mut(self.mode) = self.focus;
            self.focus = *self.remembered.pane_mut(mode);
            self.mode = mode;
        }
        if let Some(focus) = requests.focus {
            self.focus = focus;
        }
    }

    /// Move focus to one pane on the application's own behalf.
    pub(crate) fn focus_pane(&mut self, focus: ReviewPane) {
        self.focus = focus;
    }

    /// Move focus to the other pane and return the new focus.
    pub(crate) fn toggle_focus(&mut self) -> ReviewPane {
        self.focus = match self.focus {
            ReviewPane::Navigation => ReviewPane::Detail,
            ReviewPane::Detail => ReviewPane::Navigation,
        };
        self.focus
    }

    /// The component that owns keyboard focus when no modal is open.
    pub(crate) fn focused_pane(&self) -> PaneComponent {
        match self.focus {
            ReviewPane::Navigation => self.navigation_pane(),
            ReviewPane::Detail => PaneComponent::Diff,
        }
    }

    /// The component shown in the navigation pane for the current mode.
    pub(crate) fn navigation_pane(&self) -> PaneComponent {
        match self.mode {
            ReviewNavigation::Files => PaneComponent::Files,
            ReviewNavigation::Threads => PaneComponent::Threads,
            ReviewNavigation::Explore => PaneComponent::Explore,
        }
    }

    /// Whether the focused pane is shown inside another component that may
    /// claim keys first: the Explore evidence, shown inside Explore.
    pub(crate) fn focus_is_enclosed(&self) -> bool {
        self.mode == ReviewNavigation::Explore && self.focus == ReviewPane::Detail
    }

    /// The focus Explore saves with its round, while Explore is open.
    pub(crate) fn explore_focus(&self) -> Option<ReviewPane> {
        (self.mode == ReviewNavigation::Explore).then_some(self.focus)
    }

    pub(crate) fn set_modals(&mut self, modals: OpenModals) {
        self.modals = modals;
    }

    pub(crate) fn modals(&self) -> OpenModals {
        self.modals
    }

    /// The modal that takes keyboard input, if any is open.
    pub(crate) fn modal(&self) -> Option<Modal> {
        self.modals.top()
    }

    /// Recompute the layout that drawing and hit-testing share.
    pub(crate) fn relayout(&mut self, viewport: Viewport) {
        self.layout = ScreenLayout::new(viewport, self.mode, self.focus, self.modals.locations);
    }
}

/// Navigation requests published by components during one dispatch.
///
/// Only the latest mode and the latest focus count, as when several
/// components ask in the same cascade.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct NavigationRequests {
    mode: Option<ReviewNavigation>,
    focus: Option<ReviewPane>,
}

impl NavigationRequests {
    /// Hand over the pending requests and start collecting afresh.
    pub(crate) fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn mode_requested(&mut self, event: &ReviewNavigationChanged) {
        self.mode = Some(event.0);
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn focus_requested(&mut self, event: &ReviewPaneFocusRequested) {
        self.focus = Some(event.0);
    }
}

impl Component<Action> for NavigationRequests {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::mode_requested);
        subscriptions.subscribe(Self::focus_requested);
    }
}

#[cfg(test)]
#[path = "navigation.tests.rs"]
mod tests;
