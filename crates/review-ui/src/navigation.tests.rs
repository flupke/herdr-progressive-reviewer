use component_core::ComponentEventBus;
use ratatui::layout::Rect;
use ui_events::{ReviewNavigation, ReviewNavigationChanged, ReviewPane, ReviewPaneFocusRequested};

use super::{Modal, Navigation, NavigationRequests, OpenModals, PaneComponent};
use crate::Action;
use crate::layout::{Body, Viewport};

const WIDE: Viewport = Viewport {
    width: 120,
    height: 40,
    file_width: Some(40),
};

const NARROW: Viewport = Viewport {
    width: 60,
    height: 20,
    file_width: None,
};

fn requested(mode: Option<ReviewNavigation>, focus: Option<ReviewPane>) -> NavigationRequests {
    let mut bus = ComponentEventBus::<Action>::new();
    let requests = bus.mount(|_| NavigationRequests::default());
    if let Some(mode) = mode {
        bus.publish(ReviewNavigationChanged(mode)).unwrap();
    }
    if let Some(focus) = focus {
        bus.publish(ReviewPaneFocusRequested(focus)).unwrap();
    }
    bus.get_mut::<NavigationRequests>(requests).unwrap().take()
}

#[test]
fn each_mode_restores_the_focus_it_had_when_the_reviewer_left_it() {
    let mut navigation = Navigation::new(WIDE);
    navigation.focus_pane(ReviewPane::Detail);

    navigation.apply(requested(Some(ReviewNavigation::Threads), None));
    assert_eq!(navigation.focus(), ReviewPane::Navigation);

    navigation.apply(requested(Some(ReviewNavigation::Files), None));
    assert_eq!(navigation.mode(), ReviewNavigation::Files);
    assert_eq!(navigation.focus(), ReviewPane::Detail);
}

#[test]
fn a_focus_request_wins_over_the_remembered_focus_of_the_new_mode() {
    let mut navigation = Navigation::new(WIDE);

    navigation.apply(requested(
        Some(ReviewNavigation::Threads),
        Some(ReviewPane::Detail),
    ));

    assert_eq!(navigation.mode(), ReviewNavigation::Threads);
    assert_eq!(navigation.focused_pane(), PaneComponent::Diff);
    navigation.apply(requested(Some(ReviewNavigation::Files), None));
    navigation.apply(requested(Some(ReviewNavigation::Threads), None));
    assert_eq!(navigation.focus(), ReviewPane::Detail);
}

#[test]
fn taking_requests_consumes_them() {
    let mut requests = requested(Some(ReviewNavigation::Explore), Some(ReviewPane::Detail));

    assert_ne!(requests.take(), NavigationRequests::default());
    assert_eq!(requests.take(), NavigationRequests::default());
}

#[test]
fn explore_evidence_focus_is_enclosed_by_explore() {
    let mut navigation = Navigation::new(WIDE);
    navigation.apply(requested(Some(ReviewNavigation::Explore), None));
    assert_eq!(navigation.focused_pane(), PaneComponent::Explore);
    assert!(!navigation.focus_is_enclosed());
    assert_eq!(navigation.explore_focus(), Some(ReviewPane::Navigation));

    navigation.toggle_focus();

    assert_eq!(navigation.focused_pane(), PaneComponent::Diff);
    assert!(navigation.focus_is_enclosed());
    assert_eq!(navigation.explore_focus(), Some(ReviewPane::Detail));
    navigation.apply(requested(Some(ReviewNavigation::Files), None));
    assert!(!navigation.focus_is_enclosed());
    assert_eq!(navigation.explore_focus(), None);
}

#[test]
fn the_revision_picker_covers_the_overlay_and_the_location_selector() {
    let mut navigation = Navigation::new(WIDE);
    assert_eq!(navigation.modal(), None);

    navigation.set_modals(OpenModals {
        revision: true,
        overlay: true,
        locations: true,
    });
    assert_eq!(navigation.modal(), Some(Modal::Revision));
    navigation.set_modals(OpenModals {
        revision: false,
        overlay: true,
        locations: true,
    });
    assert_eq!(navigation.modal(), Some(Modal::Overlay));
    navigation.set_modals(OpenModals {
        locations: true,
        ..OpenModals::default()
    });
    assert_eq!(navigation.modal(), Some(Modal::Locations));
}

#[test]
fn a_wide_layout_puts_the_tabs_on_the_navigation_pane_beside_the_diff() {
    let mut navigation = Navigation::new(WIDE);
    navigation.relayout(WIDE);
    let layout = navigation.layout();

    assert_eq!(layout.navigation_pane(), Some(Rect::new(0, 1, 40, 38)));
    assert_eq!(
        layout.navigation_input_area(),
        Some(Rect::new(1, 2, 38, 36))
    );
    assert_eq!(layout.tabs(), Some(Rect::new(1, 1, 38, 1)));
    assert_eq!(layout.visible_diff_pane(), Some(Rect::new(40, 1, 80, 38)));
    assert!(layout.is_separator(40, 5));
    assert_eq!(layout.footer(), Rect::new(0, 39, 120, 1));
}

#[test]
fn a_narrow_layout_shows_only_the_focused_pane() {
    let mut navigation = Navigation::new(NARROW);
    navigation.relayout(NARROW);
    assert_eq!(navigation.layout().visible_diff_pane(), None);
    assert!(navigation.layout().tabs().is_some());

    navigation.toggle_focus();
    navigation.relayout(NARROW);

    let layout = navigation.layout();
    assert_eq!(layout.navigation_pane(), None);
    assert_eq!(layout.tabs(), None);
    assert_eq!(layout.visible_diff_pane(), Some(Rect::new(0, 1, 60, 18)));
    assert!(matches!(
        layout.body(),
        Body::Single {
            pane: ReviewPane::Detail,
            ..
        }
    ));
}

#[test]
fn explore_fills_the_body_beside_an_open_location_selector() {
    let mut navigation = Navigation::new(WIDE);
    navigation.apply(requested(Some(ReviewNavigation::Explore), None));
    navigation.relayout(WIDE);
    assert_eq!(
        navigation.layout().navigation_pane(),
        Some(Rect::new(0, 1, 120, 38))
    );
    assert_eq!(navigation.layout().diff_pane(), None);

    navigation.set_modals(OpenModals {
        locations: true,
        ..OpenModals::default()
    });
    navigation.relayout(WIDE);

    let layout = navigation.layout();
    let locations = layout.locations_pane();
    let source = layout.navigation_pane().unwrap();
    assert_eq!(locations.right(), source.x);
    assert_eq!(source.right(), 120);
    assert_eq!(layout.navigation_input_area(), Some(source));
}

#[test]
fn a_layout_fitted_to_another_area_keeps_its_state_and_moves_with_the_area() {
    let mut navigation = Navigation::new(WIDE);
    navigation.relayout(WIDE);
    let layout = navigation.layout();
    assert_eq!(layout.fitted_to(Rect::new(0, 0, 120, 40)), layout);

    let moved = layout.fitted_to(Rect::new(2, 3, 120, 40));

    assert_eq!(moved.header(), Rect::new(2, 3, 120, 1));
    assert_eq!(moved.footer(), Rect::new(2, 42, 120, 1));
    assert_eq!(moved.navigation_pane(), Some(Rect::new(2, 4, 40, 38)));
}
