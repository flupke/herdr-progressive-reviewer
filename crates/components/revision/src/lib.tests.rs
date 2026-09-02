use component_core::{ComponentEventBus, EventEnvelope};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use review_guide::ReviewCheckpoint;
use review_repository::repository::{
    ChangeId, RevisionCandidate, RevisionDirection, RevisionHistoryLine,
};
use review_types::ReviewUnit;
use ui_events::{
    CurrentReviewLocationChanged, RepositoryFilesChanged, RepositoryRefreshFinished,
    RepositoryRefreshStarted, ReviewLocation, RevisionCandidatesLoaded, RevisionEditFailed,
    RevisionHistoryLoadId, RevisionHistoryLoaded,
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
        .dispatch_global_input(&EventEnvelope::new(Key::Char('v')))
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
    send_key(&mut bus, Key::Char('v'));
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
fn selector_shortcut_loads_the_rendered_revision_history() {
    let (mut bus, target) = mounted_component();
    set_current_revision(&mut bus);

    send_key(&mut bus, Key::Char('v'));
    let results = bus
        .dispatch_global_input(&EventEnvelope::new(Key::Char('v')))
        .unwrap()
        .into_results();
    assert_eq!(
        actions(results),
        [ui_actions::Action::LoadRevisionHistory {
            load_id: history_load_id(0),
        }]
    );

    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![history_line("parent"), history_line("child")]),
    })
    .unwrap();
    let screen = rendered_component(&bus, target);
    assert!(screen.contains("Select revision"));
    assert!(screen.contains("parent description"));
    assert!(screen.contains("child description"));
}

#[test]
fn selector_opens_while_the_initial_repository_refresh_is_running() {
    let (mut bus, target) = mounted_component();
    bus.publish(RepositoryRefreshStarted).unwrap();

    send_key(&mut bus, Key::Char('v'));
    let results = bus
        .dispatch_global_input(&EventEnvelope::new(Key::Char('v')))
        .unwrap()
        .into_results();

    assert_eq!(
        actions(results),
        [ui_actions::Action::LoadRevisionHistory {
            load_id: history_load_id(0),
        }]
    );
    assert!(rendered_component(&bus, target).contains("Loading revision history"));

    bus.publish(repository_files_changed("current")).unwrap();
    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![current_history_line("current")]),
    })
    .unwrap();

    assert!(rendered_component(&bus, target).contains("current description"));
}

#[test]
fn repository_and_late_worker_events_do_not_close_an_open_selector() {
    let (mut bus, target) = mounted_component();
    set_current_revision(&mut bus);
    send_key(&mut bus, Key::Char('v'));
    send_key(&mut bus, Key::Char('v'));
    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![current_history_line("current")]),
    })
    .unwrap();

    bus.publish(repository_files_changed("current")).unwrap();
    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![current_history_line("late")]),
    })
    .unwrap();
    bus.publish(RevisionEditFailed {
        message: Some("late edit failure".to_owned()),
    })
    .unwrap();

    let screen = rendered_component(&bus, target);
    assert!(screen.contains("current description"));
    assert!(!screen.contains("late description"));
}

#[test]
fn a_closed_history_request_cannot_replace_the_next_selector() {
    let (mut bus, target) = mounted_component();
    set_current_revision(&mut bus);

    send_key(&mut bus, Key::Char('v'));
    assert_eq!(
        actions(
            bus.dispatch_global_input(&EventEnvelope::new(Key::Char('v')))
                .unwrap()
                .into_results()
        ),
        [ui_actions::Action::LoadRevisionHistory {
            load_id: history_load_id(0),
        }]
    );
    send_key(&mut bus, Key::Escape);
    send_key(&mut bus, Key::Char('v'));
    assert_eq!(
        actions(
            bus.dispatch_global_input(&EventEnvelope::new(Key::Char('v')))
                .unwrap()
                .into_results()
        ),
        [ui_actions::Action::LoadRevisionHistory {
            load_id: history_load_id(1),
        }]
    );

    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Err("late failure".to_owned()),
    })
    .unwrap();
    assert!(rendered_component(&bus, target).contains("Loading revision history"));

    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(1),
        result: Ok(vec![current_history_line("current")]),
    })
    .unwrap();
    assert!(rendered_component(&bus, target).contains("current description"));
}

#[test]
fn slash_fuzzy_matches_a_typed_change_id() {
    let (mut bus, target) = mounted_component();
    set_current_revision(&mut bus);
    send_key(&mut bus, Key::Char('v'));
    send_key(&mut bus, Key::Char('v'));
    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![history_line("alpha"), history_line("rykwutkz")]),
    })
    .unwrap();

    send_key(&mut bus, Key::Char('/'));
    for character in "rkt".chars() {
        send_key(&mut bus, Key::Char(character));
    }

    assert!(rendered_component(&bus, target).contains("Select revision: /rkt"));
    let results = bus
        .dispatch_global_input(&EventEnvelope::new(Key::Enter))
        .unwrap()
        .into_results();
    assert_eq!(
        actions(results),
        [ui_actions::Action::EditRevision {
            change_id: ChangeId::from("rykwutkz".to_owned())
        }]
    );
}

#[test]
fn visible_short_change_id_prefix_has_search_priority() {
    let (mut bus, _) = mounted_component();
    set_current_revision(&mut bus);
    send_key(&mut bus, Key::Char('v'));
    send_key(&mut bus, Key::Char('v'));
    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![
            history_line_with_text("surpvzo-hidden", "surpvslz", "geometry update"),
            history_line_with_text("description-match", "abcdefgh", "very zesty orange"),
            history_line_with_text("target-full-id", "vzolwzov", "target revision"),
        ]),
    })
    .unwrap();

    send_key(&mut bus, Key::Char('/'));
    for character in "vzo".chars() {
        send_key(&mut bus, Key::Char(character));
    }
    let results = bus
        .dispatch_global_input(&EventEnvelope::new(Key::Enter))
        .unwrap()
        .into_results();

    assert_eq!(
        actions(results),
        [ui_actions::Action::EditRevision {
            change_id: ChangeId::from("target-full-id".to_owned())
        }]
    );
}

#[test]
fn best_visible_short_change_id_prefix_updates_after_each_character() {
    let (mut bus, target) = mounted_component();
    set_current_revision(&mut bus);
    send_key(&mut bus, Key::Char('v'));
    send_key(&mut bus, Key::Char('v'));
    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![
            history_line_with_text("v-first", "vabcdefg", "first prefix"),
            history_line_with_text("vz-second", "vzabcdef", "second prefix"),
            history_line_with_text("vzo-third", "vzolwzov", "third prefix"),
        ]),
    })
    .unwrap();
    send_key(&mut bus, Key::Char('/'));

    send_key(&mut bus, Key::Char('v'));
    assert!(selected_history_text(&bus, target).contains("vabcdefg first prefix"));

    send_key(&mut bus, Key::Char('z'));
    assert!(selected_history_text(&bus, target).contains("vzabcdef second prefix"));

    send_key(&mut bus, Key::Char('o'));
    assert!(selected_history_text(&bus, target).contains("vzolwzov third prefix"));
}

#[test]
fn an_unmatched_fuzzy_query_cannot_select_the_previous_revision() {
    let (mut bus, _) = mounted_component();
    set_current_revision(&mut bus);
    send_key(&mut bus, Key::Char('v'));
    send_key(&mut bus, Key::Char('v'));
    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![history_line("alpha"), history_line("rykwutkz")]),
    })
    .unwrap();
    send_key(&mut bus, Key::Char('/'));
    for character in "missing".chars() {
        send_key(&mut bus, Key::Char(character));
    }

    let results = bus
        .dispatch_global_input(&EventEnvelope::new(Key::Enter))
        .unwrap()
        .into_results();

    assert!(actions(results).is_empty());
}

#[test]
fn escape_stops_filtering_before_it_closes_the_revision_selector() {
    let (mut bus, target) = mounted_component();
    set_current_revision(&mut bus);
    send_key(&mut bus, Key::Char('v'));
    send_key(&mut bus, Key::Char('v'));
    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![history_line("alpha"), history_line("rykwutkz")]),
    })
    .unwrap();
    send_key(&mut bus, Key::Char('/'));
    send_key(&mut bus, Key::Char('r'));

    send_key(&mut bus, Key::Escape);
    let screen = rendered_component(&bus, target);
    assert!(screen.contains("Select revision"));
    assert!(!screen.contains("Select revision: /r"));

    send_key(&mut bus, Key::Escape);
    assert!(!rendered_component(&bus, target).contains("Select revision"));
}

#[test]
fn revision_history_starts_on_and_highlights_the_current_revision() {
    let (mut bus, target) = mounted_component();
    set_current_revision(&mut bus);
    send_key(&mut bus, Key::Char('v'));
    send_key(&mut bus, Key::Char('v'));
    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![
            history_line("parent"),
            current_history_line("current"),
        ]),
    })
    .unwrap();

    let component = bus.get::<RevisionComponent>(target).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
    terminal
        .draw(|frame| component.render(frame.area(), frame.buffer_mut()))
        .unwrap();
    assert_eq!(
        terminal.backend().buffer().cell((7, 6)).unwrap().bg,
        Theme::default().palette.selection
    );
    assert_eq!(
        terminal.backend().buffer().cell((52, 6)).unwrap().bg,
        Theme::default().palette.selection
    );

    let results = bus
        .dispatch_global_input(&EventEnvelope::new(Key::Enter))
        .unwrap()
        .into_results();
    assert_eq!(
        actions(results),
        [ui_actions::Action::EditRevision {
            change_id: ChangeId::from("current".to_owned())
        }]
    );
}

#[test]
fn j_and_k_move_between_revision_history_rows() {
    let (mut bus, _) = mounted_component();
    set_current_revision(&mut bus);
    send_key(&mut bus, Key::Char('v'));
    send_key(&mut bus, Key::Char('v'));
    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![
            history_line("parent"),
            current_history_line("current"),
            history_line("child"),
        ]),
    })
    .unwrap();

    send_key(&mut bus, Key::Char('k'));
    send_key(&mut bus, Key::Char('j'));
    send_key(&mut bus, Key::Char('j'));
    let results = bus
        .dispatch_global_input(&EventEnvelope::new(Key::Enter))
        .unwrap()
        .into_results();

    assert_eq!(
        actions(results),
        [ui_actions::Action::EditRevision {
            change_id: ChangeId::from("child".to_owned())
        }]
    );
}

#[test]
fn navigation_skips_immutable_revision_history_rows() {
    let (mut bus, _) = mounted_component();
    set_current_revision(&mut bus);
    send_key(&mut bus, Key::Char('v'));
    send_key(&mut bus, Key::Char('v'));
    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![
            immutable_history_line("immutable"),
            current_history_line("current"),
            history_line("child"),
        ]),
    })
    .unwrap();

    send_key(&mut bus, Key::Char('k'));
    let results = bus
        .dispatch_global_input(&EventEnvelope::new(Key::Enter))
        .unwrap()
        .into_results();

    assert_eq!(
        actions(results),
        [ui_actions::Action::EditRevision {
            change_id: ChangeId::from("current".to_owned())
        }]
    );
}

#[test]
fn revision_history_keeps_jj_terminal_colors() {
    let (mut bus, target) = mounted_component();
    set_current_revision(&mut bus);
    send_key(&mut bus, Key::Char('v'));
    send_key(&mut bus, Key::Char('v'));
    bus.publish(RevisionHistoryLoaded {
        load_id: history_load_id(0),
        result: Ok(vec![history_line("alpha")]),
    })
    .unwrap();

    let component = bus.get::<RevisionComponent>(target).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
    terminal
        .draw(|frame| component.render(frame.area(), frame.buffer_mut()))
        .unwrap();

    let first_history_cell = terminal.backend().buffer().cell((7, 5)).unwrap();
    assert_eq!(first_history_cell.symbol(), "a");
    assert_eq!(first_history_cell.fg, Color::Magenta);
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

fn history_load_id(value: u64) -> RevisionHistoryLoadId {
    RevisionHistoryLoadId::new(value)
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
        bus.dispatch_global_input(&EventEnvelope::new(Key::Char('v')))
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

fn selected_history_text(
    bus: &ComponentEventBus<ui_actions::Action>,
    target: component_core::ComponentTarget,
) -> String {
    let component = bus.get::<RevisionComponent>(target).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
    terminal
        .draw(|frame| component.render(frame.area(), frame.buffer_mut()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let selection = Theme::default().palette.selection;
    (0..buffer.area.height)
        .find(|row| {
            (0..buffer.area.width).any(|column| {
                buffer
                    .cell((column, *row))
                    .is_some_and(|cell| cell.bg == selection)
            })
        })
        .map(|row| {
            (0..buffer.area.width)
                .filter_map(|column| buffer.cell((column, row)))
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        })
        .unwrap_or_default()
}

fn candidate(id: &str) -> RevisionCandidate {
    RevisionCandidate {
        change_id: ChangeId::from(id.to_owned()),
        short_change_id: id.to_owned(),
        description: format!("{id} description"),
    }
}

fn history_line(id: &str) -> RevisionHistoryLine {
    history_line_with_text(id, id, &format!("{id} description"))
}

fn history_line_with_text(full_id: &str, short_id: &str, description: &str) -> RevisionHistoryLine {
    RevisionHistoryLine {
        text: format!("\u{1b}[35m{short_id}\u{1b}[39m {description}"),
        plain_text: format!("{short_id} {description}"),
        short_change_id: Some(short_id.to_owned()),
        change_id: Some(ChangeId::from(full_id.to_owned())),
        is_current: false,
        is_immutable: false,
    }
}

fn immutable_history_line(id: &str) -> RevisionHistoryLine {
    RevisionHistoryLine {
        is_immutable: true,
        ..history_line(id)
    }
}

fn current_history_line(id: &str) -> RevisionHistoryLine {
    RevisionHistoryLine {
        is_current: true,
        ..history_line(id)
    }
}
