use component_core::{ComponentEventBus, ComponentSubscriptions, ComponentTarget, EventEnvelope};
use ratatui::{Terminal, backend::TestBackend, style::Color};
use review_guide::ReviewCheckpoint;
use review_repository::repository::{ChangedFile, DiffStatistics, Repository};
use review_state::ReviewStatus;
use review_test_support::{GitFixture, ReviewRepositoryFixture, complete_repository_snapshot};
use ui_actions::Action;
use ui_events::{
    FileSelected, FileSummary, FilesOverviewChanged, FilesViewportChanged, GuidePathsChanged,
    PointerInput, PointerInputKind, PointerPosition, RepositoryFilesChanged, ReviewStateSaved,
};
use ui_shortcuts::Key;

use super::FilesComponent;
use ui_theme::{Palette, Theme};

fn changed_files(paths: &[&str]) -> Vec<ChangedFile> {
    let fixture = GitFixture::new();
    for path in paths {
        fixture.write(path, b"changed\ncontent\n");
    }
    let state = tempfile::tempdir().expect("state directory must be created");
    let repository = Repository::discover(fixture.root())
        .expect("fixture repository must be discovered")
        .with_state_root(state.path());
    complete_repository_snapshot(&repository).files
}

#[test]
fn repository_and_keyboard_inputs_publish_the_selected_file() {
    let mut registry = ComponentEventBus::<Action>::new();
    let target = registry.mount(FilesComponent::new);
    registry.mount(|_| SelectionOutput);
    let files = changed_files(&["first.rs", "second.rs"]);
    let changed = RepositoryFilesChanged {
        review_checkpoint: ReviewCheckpoint::new("change", "commit"),
        files: files
            .iter()
            .map(|file| FileSummary::from_changed(file, ReviewStatus::Unreviewed))
            .collect(),
    };
    let initial = registry
        .publish_envelope(EventEnvelope::new(changed))
        .expect("repository event must dispatch")
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .collect::<Vec<_>>();
    assert_eq!(
        initial,
        [Action::Output {
            text: "selected:first.rs".to_owned(),
        }]
    );

    let moved = registry
        .dispatch_input(&EventEnvelope::new(Key::Down), target)
        .expect("keyboard event must dispatch")
        .into_results()
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .collect::<Vec<_>>();
    assert_eq!(
        moved,
        [Action::Output {
            text: "selected:second.rs".to_owned(),
        }]
    );
}

#[test]
fn global_shortcuts_move_between_files_that_need_review() {
    let mut registry = ComponentEventBus::<Action>::new();
    registry.mount(FilesComponent::new);
    let other_target = registry.mount(|_| SelectionOutput);
    registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "commit"),
            files: vec![
                FileSummary::new("first.rs", ReviewStatus::Unreviewed),
                FileSummary::new("second.rs", ReviewStatus::Reviewed),
                FileSummary::new("third.rs", ReviewStatus::ChangedSinceReview),
            ],
        }))
        .expect("repository event must dispatch");

    for (first, second, expected_path) in [
        ('[', 'f', "third.rs"),
        (']', 'f', "first.rs"),
        (']', 'f', "third.rs"),
    ] {
        let prefix = registry
            .dispatch_input(&EventEnvelope::new(Key::Char(first)), other_target)
            .expect("shortcut prefix must dispatch");
        assert!(prefix.global_input_pending());
        let actions = registry
            .dispatch_input(&EventEnvelope::new(Key::Char(second)), other_target)
            .expect("shortcut must dispatch")
            .into_results()
            .into_iter()
            .flat_map(component_core::DispatchResult::into_actions)
            .collect::<Vec<_>>();
        assert_eq!(
            actions,
            [Action::Output {
                text: format!("selected:{expected_path}"),
            }]
        );
    }
}

#[test]
fn global_unreviewed_file_shortcuts_do_nothing_before_files_arrive() {
    let mut registry = ComponentEventBus::<Action>::new();
    registry.mount(FilesComponent::new);
    let other_target = registry.mount(|_| SelectionOutput);

    for first in ['[', ']'] {
        registry
            .dispatch_input(&EventEnvelope::new(Key::Char(first)), other_target)
            .expect("shortcut prefix must dispatch");
        let actions = registry
            .dispatch_input(&EventEnvelope::new(Key::Char('f')), other_target)
            .expect("shortcut must dispatch")
            .into_results()
            .into_iter()
            .flat_map(component_core::DispatchResult::into_actions)
            .collect::<Vec<_>>();
        assert!(actions.is_empty());
    }
}

#[test]
fn moving_back_to_the_first_file_reveals_its_parent_directory() {
    let mut registry = ComponentEventBus::<Action>::new();
    let target = registry.mount(FilesComponent::new);
    registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "commit"),
            files: vec![
                FileSummary::new("parent/first.rs", ReviewStatus::Unreviewed),
                FileSummary::new("second.rs", ReviewStatus::Unreviewed),
            ],
        }))
        .expect("repository event must dispatch");
    registry
        .publish_envelope(EventEnvelope::new(FilesViewportChanged { rows: 2 }))
        .expect("viewport event must dispatch");

    for delta in [1, -1] {
        registry
            .dispatch_hovered_input(
                &EventEnvelope::new(PointerInput {
                    kind: PointerInputKind::Scroll(delta),
                    position: None,
                }),
                target,
            )
            .expect("pointer input must dispatch");
    }

    let rendered = rendered_files(&registry, target);
    assert!(rendered.contains("parent/"), "{rendered:?}");
    assert!(rendered.contains("first.rs"), "{rendered:?}");
}

#[test]
fn rendering_shows_review_state_and_line_statistics() {
    let mut registry = ComponentEventBus::<Action>::new();
    let target = registry.mount(FilesComponent::new);
    let changed_file = changed_files(&["src/lib.rs"]).remove(0);
    let mut file_summary = FileSummary::from_changed(&changed_file, ReviewStatus::Unreviewed);
    file_summary.file.statistics = DiffStatistics {
        lines_added: 20,
        lines_removed: 10,
    };
    file_summary.review_state.current_diff_statistics = DiffStatistics {
        lines_added: 2,
        lines_removed: 0,
    };
    registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "commit"),
            files: vec![file_summary],
        }))
        .expect("repository event must dispatch");
    registry
        .publish_envelope(EventEnvelope::new(FilesViewportChanged { rows: 4 }))
        .expect("viewport event must dispatch");
    let component = registry
        .get::<FilesComponent>(target)
        .expect("files component must be mounted");
    let mut terminal = Terminal::new(TestBackend::new(30, 4)).expect("terminal must open");
    terminal
        .draw(|frame| {
            component.render(
                frame.area(),
                frame.buffer_mut(),
                Palette {
                    focus: Color::White,
                    dim: Color::DarkGray,
                    cursor: Color::Blue,
                    warning: Color::Yellow,
                    insertion: Color::Green,
                    deletion: Color::Red,
                    guide: Color::Yellow,
                    ..Theme::default().palette
                },
                true,
            );
        })
        .expect("render must succeed");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("○ lib.rs"), "{rendered:?}");
    assert!(rendered.contains("+2"), "{rendered:?}");
}

#[test]
fn repository_event_updates_the_header_overview() {
    let mut registry = ComponentEventBus::<Action>::new();
    registry.mount(FilesComponent::new);
    registry.mount(|_| OverviewOutput);
    let mut first = FileSummary::new("first.rs", ReviewStatus::Reviewed);
    first.file.statistics = DiffStatistics {
        lines_added: 5,
        lines_removed: 2,
    };
    let mut second = FileSummary::new("second.rs", ReviewStatus::Unreviewed);
    second.file.statistics = DiffStatistics {
        lines_added: 3,
        lines_removed: 4,
    };
    let actions = registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new(review_types::ReviewUnit::default(), "commit"),
            files: vec![first, second],
        }))
        .expect("repository event must dispatch")
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .collect::<Vec<_>>();

    assert_eq!(
        actions,
        [Action::Output {
            text: "overview:1/2:+8:-6".to_owned(),
        }]
    );
}

#[test]
fn review_input_moves_optimistically_and_failure_restores_the_status() {
    let mut registry = ComponentEventBus::<Action>::new();
    let target = registry.mount(FilesComponent::new);
    registry.mount(|_| SelectionOutput);
    registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "commit"),
            files: vec![
                FileSummary::new("first.rs", ReviewStatus::Unreviewed),
                FileSummary::new("second.rs", ReviewStatus::Unreviewed),
            ],
        }))
        .expect("repository event must dispatch");
    registry
        .publish_envelope(EventEnvelope::new(FilesViewportChanged { rows: 4 }))
        .expect("viewport event must dispatch");

    let review = registry
        .dispatch_input(&EventEnvelope::new(Key::Space), target)
        .expect("review input must dispatch")
        .into_results()
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .collect::<Vec<_>>();
    assert_eq!(
        review,
        [
            Action::SetReviewed {
                path: "first.rs".to_owned(),
                reviewed: true,
            },
            Action::Output {
                text: "selected:second.rs".to_owned(),
            },
        ]
    );

    registry
        .publish_envelope(EventEnvelope::new(ReviewStateSaved {
            review_unit: review_types::ReviewUnit::from("change"),
            path: "first.rs".to_owned(),
            result: Err(()),
        }))
        .expect("review result must dispatch");
    let mut terminal = Terminal::new(TestBackend::new(30, 4)).expect("terminal must open");
    terminal
        .draw(|frame| {
            registry
                .get::<FilesComponent>(target)
                .expect("files component must be mounted")
                .render(frame.area(), frame.buffer_mut(), palette(), true);
        })
        .expect("render must succeed");
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("○ first.rs"), "{rendered:?}");
}

#[test]
fn pointer_input_can_insert_a_path_without_loading_its_diff() {
    let mut registry = ComponentEventBus::<Action>::new();
    let target = registry.mount(FilesComponent::new);
    registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "commit"),
            files: vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
        }))
        .expect("repository event must dispatch");
    registry
        .publish_envelope(EventEnvelope::new(FilesViewportChanged { rows: 4 }))
        .expect("viewport event must dispatch");
    let actions = registry
        .dispatch_hovered_input(
            &EventEnvelope::new(PointerInput {
                kind: PointerInputKind::ControlClick,
                position: Some(PointerPosition {
                    terminal_column: 2,
                    terminal_row: 2,
                    component_column: 2,
                    component_row: 1,
                }),
            }),
            target,
        )
        .expect("pointer input must dispatch")
        .into_results()
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .collect::<Vec<_>>();
    assert_eq!(
        actions,
        [Action::Output {
            text: "src/lib.rs".to_owned(),
        }]
    );
}

struct SelectionOutput;

struct OverviewOutput;

impl OverviewOutput {
    #[allow(clippy::unused_self)]
    fn changed(&mut self, event: &FilesOverviewChanged) -> Vec<Action> {
        vec![Action::Output {
            text: format!(
                "overview:{}/{}:+{}:-{}",
                event.reviewed, event.total, event.lines_added, event.lines_removed
            ),
        }]
    }
}

impl component_core::Component<Action> for OverviewOutput {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::changed);
    }
}

impl SelectionOutput {
    #[allow(clippy::unused_self)]
    fn selected(&mut self, event: &FileSelected) -> Vec<Action> {
        vec![Action::Output {
            text: format!("selected:{}", event.path),
        }]
    }
}

impl component_core::Component<Action> for SelectionOutput {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::selected);
    }
}

#[test]
fn repository_refresh_publishes_selection_from_the_files_component() {
    let mut registry = ComponentEventBus::<Action>::new();
    registry.mount(FilesComponent::new);
    registry.mount(|_| SelectionOutput);

    let actions = registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "commit"),
            files: vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
        }))
        .expect("repository event must dispatch")
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .collect::<Vec<_>>();

    assert!(actions.contains(&Action::Output {
        text: "selected:src/lib.rs".to_owned(),
    }));
}

#[test]
fn new_repository_checkpoint_removes_old_guide_decorations() {
    let mut registry = ComponentEventBus::<Action>::new();
    let target = registry.mount(FilesComponent::new);
    let files = vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)];
    registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "first"),
            files: files.clone(),
        }))
        .expect("repository event must dispatch");
    registry
        .publish_envelope(EventEnvelope::new(GuidePathsChanged {
            paths: vec!["src/lib.rs".to_owned()],
        }))
        .expect("guide event must dispatch");
    assert!(rendered_files(&registry, target).contains('💬'));

    registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "second"),
            files,
        }))
        .expect("repository event must dispatch");

    assert!(!rendered_files(&registry, target).contains('💬'));
}

fn rendered_files(registry: &ComponentEventBus<Action>, target: ComponentTarget) -> String {
    let mut terminal = Terminal::new(TestBackend::new(30, 4)).expect("terminal must open");
    terminal
        .draw(|frame| {
            registry
                .get::<FilesComponent>(target)
                .expect("files component must be mounted")
                .render(frame.area(), frame.buffer_mut(), palette(), true);
        })
        .expect("render must succeed");
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}

fn palette() -> Palette {
    Palette {
        focus: Color::White,
        dim: Color::DarkGray,
        cursor: Color::Blue,
        warning: Color::Yellow,
        insertion: Color::Green,
        deletion: Color::Red,
        guide: Color::Yellow,
        ..Theme::default().palette
    }
}
