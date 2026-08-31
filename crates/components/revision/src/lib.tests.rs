use component_core::{ComponentEventBus, EventEnvelope};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use review_guide::ReviewCheckpoint;
use review_repository::repository::{ChangeId, RevisionCandidate, RevisionDirection};
use review_types::ReviewUnit;
use ui_events::{
    CurrentReviewLocationChanged, RepositoryFilesChanged, RepositoryRefreshFinished,
    RepositoryRefreshStarted, ReviewLocation, RevisionCandidatesLoaded,
};
use ui_shortcuts::Key;
use ui_theme::Theme;

use super::RevisionComponent;

#[test]
fn parent_shortcut_requests_candidates() {
    let (mut bus, _) = mounted_component();
    bus.publish(CurrentReviewLocationChanged {
        location: Some(ReviewLocation::Revision {
            review_unit: ReviewUnit::from("current".to_owned()),
        }),
    })
    .unwrap();
    send_key(&mut bus, Key::Char('['));
    let results = bus
        .dispatch_global_input(&EventEnvelope::new(Key::Char('g')))
        .unwrap()
        .into_results();
    assert_eq!(
        results.into_iter().next().unwrap().into_actions(),
        vec![ui_actions::Action::LoadRevisionCandidates(
            RevisionDirection::Parents
        )]
    );
}

#[test]
fn multiple_candidates_render_a_selector() {
    let (mut bus, target) = mounted_component();
    bus.publish(CurrentReviewLocationChanged {
        location: Some(ReviewLocation::Revision {
            review_unit: ReviewUnit::from("current".to_owned()),
        }),
    })
    .unwrap();
    send_key(&mut bus, Key::Char('['));
    send_key(&mut bus, Key::Char('g'));
    bus.publish(RevisionCandidatesLoaded {
        direction: RevisionDirection::Parents,
        result: Ok(vec![candidate("one"), candidate("two")]),
    })
    .unwrap();
    let component = bus.get::<RevisionComponent>(target).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
    terminal
        .draw(|frame| component.render(frame.area(), frame.buffer_mut()))
        .unwrap();
    let screen = terminal.backend().to_string();
    assert!(screen.contains("Select revision"));
    assert!(screen.contains("one"));
    assert!(screen.contains("two"));
}

#[test]
fn one_candidate_starts_the_revision_edit_without_a_selector() {
    let (mut bus, _) = mounted_component();
    set_current_revision(&mut bus);
    request_parent_candidates(&mut bus);

    let results = bus
        .publish(RevisionCandidatesLoaded {
            direction: RevisionDirection::Parents,
            result: Ok(vec![candidate("one")]),
        })
        .unwrap();

    assert_eq!(
        actions(results),
        [ui_actions::Action::EditRevision {
            change_id: ChangeId::from("one".to_owned()),
        }]
    );
}

#[test]
fn no_candidates_leave_navigation_ready_for_another_request() {
    let (mut bus, _) = mounted_component();
    set_current_revision(&mut bus);
    request_parent_candidates(&mut bus);
    assert!(
        actions(
            bus.publish(RevisionCandidatesLoaded {
                direction: RevisionDirection::Parents,
                result: Ok(Vec::new()),
            })
            .unwrap()
        )
        .is_empty()
    );

    request_parent_candidates(&mut bus);
    let results = bus
        .publish(RevisionCandidatesLoaded {
            direction: RevisionDirection::Parents,
            result: Ok(vec![candidate("one")]),
        })
        .unwrap();

    assert!(matches!(
        actions(results).as_slice(),
        [ui_actions::Action::EditRevision { .. }]
    ));
}

#[test]
fn escape_closes_the_revision_selector() {
    let (mut bus, target) = mounted_component();
    set_current_revision(&mut bus);
    request_parent_candidates(&mut bus);
    bus.publish(RevisionCandidatesLoaded {
        direction: RevisionDirection::Parents,
        result: Ok(vec![candidate("one"), candidate("two")]),
    })
    .unwrap();

    send_key(&mut bus, Key::Escape);

    assert!(!rendered_component(&bus, target).contains("Select revision"));
}

#[test]
fn overlapping_refreshes_block_navigation_until_all_finish() {
    let (mut bus, _) = mounted_component();
    set_current_revision(&mut bus);
    bus.publish(RepositoryRefreshStarted).unwrap();
    bus.publish(RepositoryRefreshStarted).unwrap();
    bus.publish(repository_files_changed("current")).unwrap();
    bus.publish(RepositoryRefreshFinished).unwrap();
    assert!(request_parent_candidates(&mut bus).is_empty());

    bus.publish(RepositoryRefreshFinished).unwrap();

    assert_eq!(
        request_parent_candidates(&mut bus),
        [ui_actions::Action::LoadRevisionCandidates(
            RevisionDirection::Parents
        )]
    );
}

fn repository_files_changed(review_unit: &str) -> RepositoryFilesChanged {
    RepositoryFilesChanged {
        review_checkpoint: ReviewCheckpoint::new(review_unit, "checkpoint"),
        files: Vec::new(),
    }
}

fn mounted_component() -> (
    ComponentEventBus<ui_actions::Action>,
    component_core::ComponentTarget,
) {
    let mut bus = ComponentEventBus::new();
    let target = bus.mount(|events| RevisionComponent::new(events, Theme::default().palette));
    (bus, target)
}

fn send_key(bus: &mut ComponentEventBus<ui_actions::Action>, key: Key) {
    bus.dispatch_global_input(&EventEnvelope::new(key)).unwrap();
}

fn set_current_revision(bus: &mut ComponentEventBus<ui_actions::Action>) {
    bus.publish(CurrentReviewLocationChanged {
        location: Some(ReviewLocation::Revision {
            review_unit: ReviewUnit::from("current".to_owned()),
        }),
    })
    .unwrap();
}

fn request_parent_candidates(
    bus: &mut ComponentEventBus<ui_actions::Action>,
) -> Vec<ui_actions::Action> {
    send_key(bus, Key::Char('['));
    actions(
        bus.dispatch_global_input(&EventEnvelope::new(Key::Char('g')))
            .unwrap()
            .into_results(),
    )
}

fn actions(
    results: Vec<component_core::DispatchResult<ui_actions::Action>>,
) -> Vec<ui_actions::Action> {
    results
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .collect()
}

fn rendered_component(
    bus: &ComponentEventBus<ui_actions::Action>,
    target: component_core::ComponentTarget,
) -> String {
    let component = bus.get::<RevisionComponent>(target).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
    terminal
        .draw(|frame| component.render(frame.area(), frame.buffer_mut()))
        .unwrap();
    terminal.backend().to_string()
}

fn candidate(id: &str) -> RevisionCandidate {
    RevisionCandidate {
        change_id: ChangeId::from(id.to_owned()),
        short_change_id: id.to_owned(),
        description: format!("{id} description"),
    }
}
