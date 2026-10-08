use std::path::PathBuf;
use std::time::Instant;
use ui_events::{
    AnimationTick, FileSummary, RepositoryFilesChanged, RepositoryMetadataChanged,
    RevisionCandidatesLoaded, RevisionEditFailed, RevisionHistoryLoadId, RevisionHistoryLoaded,
    SourceContentLoaded, ToastExpirationTick,
};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use review_lsp::{Event as LspEvent, Operation, SourceLocation};
use review_repository::diff::DiffRow;
use review_repository::repository::{
    ChangeId, RevisionCandidate, RevisionDirection, RevisionHistoryLine,
};
use review_source::{ReviewCheckpoint, SourceLineRange};
use review_state::ReviewStatus;
use review_types::ReviewUnit;
use toasts::ToastId;

use super::*;
use crate::Key;
use crate::{DocumentAction, DocumentLoad, LspAction, RepositoryAction, TerminalAction};
use component_core::{
    AnyInput, Component, ComponentSubscriptions, InputMatcher, InputResolution, InputScope,
};

#[path = "explore.tests.rs"]
mod explore;
#[path = "threads.tests.rs"]
mod threads;

struct SelectiveGlobalComponent;

impl Component<Action> for SelectiveGlobalComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe_input(
            InputScope::Global,
            MatchingKeys(&[Key::Char('x')]),
            Self::input,
        );
    }
}

impl SelectiveGlobalComponent {
    #[allow(clippy::unused_self)]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn input(&mut self, _input: Key) -> Vec<Action> {
        vec![Action::Lsp(LspAction::Restart)]
    }
}

struct FocusedComponent;

impl Component<Action> for FocusedComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe_input(
            InputScope::Focused,
            MatchingKeys(&[Key::Char('q'), Key::Char('x')]),
            Self::input,
        );
    }
}

impl FocusedComponent {
    #[allow(clippy::unused_self)]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn input(&mut self, _input: Key) -> Vec<Action> {
        vec![Action::Terminal(TerminalAction::Quit)]
    }
}

struct GlobalComponent;

impl Component<Action> for GlobalComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe_input(InputScope::Global, AnyInput, Self::input);
    }
}

impl GlobalComponent {
    #[allow(clippy::unused_self)]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn input(&mut self, _input: Key) -> Vec<Action> {
        vec![Action::Lsp(LspAction::Restart)]
    }
}

struct MatchingKeys(&'static [Key]);

impl<C> InputMatcher<C, Key> for MatchingKeys {
    type Output = Key;

    fn resolve(&mut self, _component: &C, key: &Key) -> InputResolution<Self::Output> {
        if self.0.contains(key) {
            InputResolution::Matched(*key)
        } else {
            InputResolution::NoMatch
        }
    }
}

fn application() -> ReviewApplication {
    ReviewApplication::new(Theme::default(), Some(24), PathBuf::new())
}

fn publish_repository(
    application: &mut ReviewApplication,
    review_checkpoint: ReviewCheckpoint,
    description: String,
    files: Vec<FileSummary>,
) -> Vec<Action> {
    let mut actions = application.publish(RepositoryMetadataChanged {
        display_id: "abcd1234".to_owned(),
        review_checkpoint: review_checkpoint.clone(),
        description,
    });
    actions.extend(application.publish(RepositoryFilesChanged {
        review_checkpoint,
        files,
    }));
    actions
}

fn publish_tick(application: &mut ReviewApplication, now: Instant) -> Vec<Action> {
    let mut actions = application.publish(AnimationTick);
    actions.extend(application.publish(ToastExpirationTick { now }));
    actions
}

#[test]
fn making_the_selected_reviewed_file_unreviewed_loads_its_diff() {
    let mut application = application();
    let review_checkpoint = ReviewCheckpoint::new("change", "checkpoint");
    let initial_actions = publish_repository(
        &mut application,
        review_checkpoint.clone(),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Reviewed)],
    );
    assert!(!initial_actions.iter().any(|action| matches!(
        action,
        Action::Document(DocumentAction::Load(DocumentLoad::Diff { .. }))
    )));

    let actions = application.update(UserInput::Key(Key::Char(' ')));

    assert_eq!(
        actions,
        vec![
            Action::Repository(RepositoryAction::SetReviewed {
                path: "src/lib.rs".to_owned(),
                reviewed: false,
            }),
            Action::Document(DocumentAction::Load(DocumentLoad::Diff {
                review_checkpoint,
                path: "src/lib.rs".to_owned(),
            })),
        ]
    );
}

#[test]
fn space_marks_the_selected_file_reviewed_from_either_pane() {
    for focus_diff in [false, true] {
        let mut application = application();
        publish_repository(
            &mut application,
            ReviewCheckpoint::new("change", "checkpoint"),
            String::new(),
            vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
        );
        if focus_diff {
            application.update(UserInput::Key(Key::Tab));
        }

        assert_eq!(
            application.update(UserInput::Key(Key::Char(' '))),
            vec![Action::Repository(RepositoryAction::SetReviewed {
                path: "src/lib.rs".to_owned(),
                reviewed: true,
            })],
        );
    }
}

#[test]
fn rf_requests_jev_review_from_files_and_diff_without_optimistic_marks() {
    for focus_diff in [false, true] {
        let mut application = application();
        let checkpoint = ReviewCheckpoint::new("change", "checkpoint");
        publish_repository(
            &mut application,
            checkpoint.clone(),
            String::new(),
            vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
        );
        if focus_diff {
            application.update(UserInput::Key(Key::Tab));
        }
        assert!(application.update(UserInput::Key(Key::Alt('a'))).is_empty());
        assert!(
            application
                .update(UserInput::Key(Key::Char('r')))
                .is_empty()
        );
        assert_eq!(
            application.update(UserInput::Key(Key::Char('f'))),
            vec![Action::Repository(RepositoryAction::AutoReview(checkpoint))],
        );
        assert!(
            application
                .update(UserInput::Key(Key::Char(' ')))
                .contains(&Action::Repository(RepositoryAction::SetReviewed {
                    path: "src/lib.rs".into(),
                    reviewed: true,
                }))
        );
    }
}

#[test]
fn unreview_all_requires_explicit_confirmation_from_either_pane() {
    for focus_diff in [false, true] {
        for answer in [Key::Char('y'), Key::Char('n'), Key::Escape] {
            let mut application = application();
            let checkpoint = ReviewCheckpoint::new("change", "checkpoint");
            publish_repository(
                &mut application,
                checkpoint.clone(),
                String::new(),
                vec![FileSummary::new("src/lib.rs", ReviewStatus::Reviewed)],
            );
            if focus_diff {
                application.update(UserInput::Key(Key::Tab));
            }
            assert!(
                application
                    .update(UserInput::Key(Key::Char('r')))
                    .is_empty()
            );
            assert!(
                application
                    .update(UserInput::Key(Key::Char('U')))
                    .is_empty()
            );
            let screen = rendered_application(&application);
            assert!(screen.contains("Set all files to unreviewed?"));
            assert!(screen.contains("[y] Yes") && screen.contains("[n] No"));
            for key in [Key::Char(' '), Key::Enter, Key::Char('q'), Key::Char('f')] {
                assert!(application.update(UserInput::Key(key)).is_empty());
                assert!(
                    rendered_application(&application).contains("Set all files to unreviewed?")
                );
            }
            let actions = application.update(UserInput::Key(answer));
            if answer == Key::Char('y') {
                assert_eq!(
                    actions,
                    vec![Action::Repository(RepositoryAction::UnreviewAll(
                        checkpoint
                    ))]
                );
            } else {
                assert!(actions.is_empty());
            }
            assert!(!rendered_application(&application).contains("Set all files to unreviewed?"));
            assert!(
                application
                    .update(UserInput::Key(Key::Char('y')))
                    .is_empty()
            );
        }
    }
}

#[test]
fn a_changed_comparison_dismisses_the_unreview_confirmation() {
    let mut application = application();
    for checkpoint in ["before", "after"] {
        publish_repository(
            &mut application,
            ReviewCheckpoint::new("change", checkpoint),
            String::new(),
            vec![FileSummary::new("src/lib.rs", ReviewStatus::Reviewed)],
        );
        if checkpoint == "before" {
            application.update(UserInput::Key(Key::Char('r')));
            application.update(UserInput::Key(Key::Char('U')));
            assert!(rendered_application(&application).contains("Set all files to unreviewed?"));
        }
    }
    assert!(!rendered_application(&application).contains("Set all files to unreviewed?"));
    assert!(
        application
            .update(UserInput::Key(Key::Char('y')))
            .is_empty()
    );
}

#[test]
fn revision_navigation_works_while_the_files_pane_has_focus() {
    let mut application = application();
    publish_repository(
        &mut application,
        ReviewCheckpoint::new(ReviewUnit::from("change"), "commit".to_owned()),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );

    assert!(
        application
            .update(UserInput::Key(Key::Char('[')))
            .is_empty()
    );
    let actions = application.update(UserInput::Key(Key::Char('v')));
    assert_eq!(
        actions,
        vec![Action::Repository(
            RepositoryAction::LoadRevisionCandidates(RevisionDirection::Parents)
        )]
    );
}

#[test]
fn revision_selector_requests_rendered_history() {
    let mut application = application();
    publish_repository(
        &mut application,
        ReviewCheckpoint::new(ReviewUnit::from("change"), "commit".to_owned()),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );

    assert!(
        application
            .update(UserInput::Key(Key::Char('v')))
            .is_empty()
    );
    assert_eq!(
        application.update(UserInput::Key(Key::Char('v'))),
        [Action::Repository(RepositoryAction::LoadRevisionHistory {
            load_id: RevisionHistoryLoadId::new(0),
        })]
    );
}

#[test]
fn lsp_startup_does_not_close_the_revision_selector() {
    let mut application = application();
    publish_repository(
        &mut application,
        ReviewCheckpoint::new(ReviewUnit::from("change"), "commit".to_owned()),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    application.update(UserInput::Key(Key::Char('v')));
    application.update(UserInput::Key(Key::Char('v')));
    application.publish(RevisionHistoryLoaded {
        load_id: RevisionHistoryLoadId::new(0),
        result: Ok(vec![RevisionHistoryLine {
            text: "change current revision".to_owned(),
            plain_text: "change current revision".to_owned(),
            graph_end: None,
            short_change_id: Some("change".to_owned()),
            change_id: Some(ChangeId::from("change".to_owned())),
            commit_id: None,
            is_current: true,
            is_immutable: false,
        }]),
    });

    application.publish(LspEvent::Initializing(review_lsp::ServerStartup {
        id: ToastId::generate(),
        name: "rust-analyzer",
    }));

    assert!(rendered_application(&application).contains("Select revision"));
    assert!(rendered_application(&application).contains("current revision"));
}

#[test]
fn clicking_the_change_id_opens_the_revision_selector_and_the_title_the_commit_message() {
    let mut application = application();
    publish_repository(
        &mut application,
        ReviewCheckpoint::new(ReviewUnit::from("change"), "commit".to_owned()),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );

    // The header reads " abcd1234  Commit title": the change ID from column 1. A click falls
    // on a drawn header.
    rendered_application(&application);
    let actions = application.update(UserInput::MouseClick { column: 1, row: 0 });
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::Repository(RepositoryAction::LoadRevisionHistory { .. })
    )));
    application.publish(RevisionHistoryLoaded {
        load_id: RevisionHistoryLoadId::new(0),
        result: Ok(vec![RevisionHistoryLine {
            text: "change current revision".to_owned(),
            plain_text: "change current revision".to_owned(),
            graph_end: None,
            short_change_id: Some("change".to_owned()),
            change_id: Some(ChangeId::from("change".to_owned())),
            commit_id: None,
            is_current: true,
            is_immutable: false,
        }]),
    });
    assert!(rendered_application(&application).contains("Select revision"));
    assert!(!rendered_application(&application).contains("Commit message"));
    application.update(UserInput::Key(Key::Escape));
    assert!(!rendered_application(&application).contains("Select revision"));

    // The gap between them opens neither; the title opens the commit message.
    application.update(UserInput::MouseClick { column: 10, row: 0 });
    assert!(!rendered_application(&application).contains("Commit message"));
    application.update(UserInput::MouseClick { column: 11, row: 0 });
    assert!(rendered_application(&application).contains("Commit message"));
    assert!(!rendered_application(&application).contains("Select revision"));
}

#[test]
fn clicking_outside_the_revision_selector_closes_it() {
    let mut application = application();
    publish_repository(
        &mut application,
        ReviewCheckpoint::new(ReviewUnit::from("change"), "commit".to_owned()),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    application.update(UserInput::Key(Key::Char('v')));
    application.update(UserInput::Key(Key::Char('v')));
    application.publish(RevisionHistoryLoaded {
        load_id: RevisionHistoryLoadId::new(0),
        result: Ok(vec![RevisionHistoryLine {
            text: "change current revision".to_owned(),
            plain_text: "change current revision".to_owned(),
            graph_end: None,
            short_change_id: Some("change".to_owned()),
            change_id: Some(ChangeId::from("change".to_owned())),
            commit_id: None,
            is_current: true,
            is_immutable: false,
        }]),
    });
    assert!(rendered_application(&application).contains("Select revision"));

    application.update(UserInput::MouseClick { column: 0, row: 0 });

    assert!(!rendered_application(&application).contains("Select revision"));
}

#[test]
fn clicking_outside_the_help_popup_closes_it() {
    let mut application = application();
    application.update(UserInput::Key(Key::Char('?')));
    assert!(rendered_application(&application).contains("Keyboard shortcuts"));

    application.update(UserInput::MouseClick { column: 0, row: 0 });

    assert!(!rendered_application(&application).contains("Keyboard shortcuts"));
}

#[test]
fn popup_shortcuts_close_the_popup_while_the_diff_pane_has_focus() {
    let mut application = application();
    application.update(UserInput::Key(Key::Tab));

    application.update(UserInput::Key(Key::Char('?')));
    assert!(rendered_application(&application).contains("Keyboard shortcuts"));
    application.update(UserInput::Key(Key::Char('?')));
    assert!(!rendered_application(&application).contains("Keyboard shortcuts"));

    application.update(UserInput::Key(Key::Char('c')));
    assert!(rendered_application(&application).contains("Commit message"));
    application.update(UserInput::Key(Key::Char('c')));
    assert!(!rendered_application(&application).contains("Commit message"));
}

fn application_with_ordered_input_components() -> ReviewApplication {
    let mut application = application();
    application.mount_input_component_for_test(|_| SelectiveGlobalComponent, false);
    application.mount_input_component_for_test(|_| FocusedComponent, true);
    application.mount_input_component_for_test(|_| GlobalComponent, false);
    application
}

#[test]
fn unhandled_files_input_reaches_the_global_application_controller() {
    let mut application = application();

    assert_eq!(
        application.update(UserInput::Key(Key::Char('q'))),
        [Action::Terminal(TerminalAction::Quit)]
    );
}

#[test]
fn mounted_components_render_through_the_application() {
    let application = application();
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();

    terminal
        .draw(|frame| frame.render_widget(application.frame(), frame.area()))
        .unwrap();

    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("Files   Threads   Explore"));
    assert!(rendered.contains("Diff"));
}

#[test]
fn current_diff_row_stays_visible_across_the_pane_when_files_are_focused() {
    let theme = Theme::default();
    let mut application = ReviewApplication::new(theme, Some(24), PathBuf::new());
    let review_checkpoint = ReviewCheckpoint::new("change", "checkpoint");
    publish_repository(
        &mut application,
        review_checkpoint.clone(),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    application.publish(ui_events::DiffContentLoaded {
        review_checkpoint,
        path: "src/lib.rs".to_owned(),
        rows: vec![DiffRow::Add {
            new_line: 1,
            text: "+current_row".to_owned(),
        }],
        old_content: None,
        new_content: None,
        hunks: review_hunks::FileHunks::default(),
    });

    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(application.frame(), frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let (cursor_column, row) = rendered_text_position(&application, "current_row", 80, 12)
        .expect("the current diff row must be visible");

    assert_eq!(buffer[(cursor_column, row)].bg, theme.palette.cursor);
    assert_eq!(buffer[(78, row)].bg, theme.palette.cursor);
    // Diff metadata is not source text, so the `c` is source column zero.
    assert!(
        !buffer[(cursor_column, row)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
}

#[test]
fn external_source_is_added_to_and_removed_from_the_files_component() {
    let mut application = application();
    application.update(UserInput::Resize {
        width: 80,
        height: 12,
    });
    publish_repository(
        &mut application,
        ReviewCheckpoint::new(ReviewUnit::from("change"), "commit".to_owned()),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    application.publish(SourceContentLoaded {
        snapshot_id: "commit".to_owned(),
        location: SourceLocation {
            path: PathBuf::from("/outside/example.rs"),
            line: 0,
            byte_column: 0,
            end_line: 0,
            end_byte_column: 0,
        },
        content: b"fn example() {}\n".to_vec(),
        mode: crate::SourceLoadMode::External,
    });

    assert!(rendered_application(&application).contains("/outside/example.rs"));

    application.update(UserInput::MouseClick { column: 3, row: 3 });
    let rendered = rendered_application(&application);
    assert!(!rendered.contains("/outside/example.rs"), "{rendered}");
}

#[test]
fn files_component_moves_selection_and_requests_the_new_diff() {
    let mut application = application();
    let initial = publish_repository(
        &mut application,
        ReviewCheckpoint::new(ReviewUnit::from("change"), "commit".to_owned()),
        "description".to_owned(),
        vec![
            FileSummary::new("first.rs", ReviewStatus::Unreviewed),
            FileSummary::new("second.rs", ReviewStatus::Unreviewed),
        ],
    );
    assert_eq!(
        initial,
        [
            Action::Thread(review_threads::ThreadCommand::Load("change".into())),
            Action::Document(DocumentAction::Load(DocumentLoad::Diff {
                review_checkpoint: ReviewCheckpoint::new("change", "commit"),
                path: "first.rs".to_owned(),
            })),
            Action::Lsp(LspAction::OpenDocument("first.rs".into()))
        ]
    );

    assert_eq!(
        application.update(UserInput::Key(Key::Down)),
        [Action::Lsp(LspAction::OpenDocument("second.rs".into()))]
    );
    assert_eq!(
        publish_tick(&mut application, Instant::now()),
        [Action::Document(DocumentAction::Load(DocumentLoad::Diff {
            review_checkpoint: ReviewCheckpoint::new("change", "commit"),
            path: "second.rs".to_owned(),
        }))]
    );
}

#[test]
fn control_clicking_a_file_does_not_send_text() {
    let mut application = application();
    application.update(UserInput::Resize {
        width: 80,
        height: 12,
    });
    publish_repository(
        &mut application,
        ReviewCheckpoint::new(ReviewUnit::from("change"), "commit".to_owned()),
        String::new(),
        vec![FileSummary::new("lib.rs", ReviewStatus::Unreviewed)],
    );

    assert!(
        application
            .update(UserInput::MouseControlClick { column: 1, row: 2 })
            .is_empty()
    );
}

#[test]
fn matched_focused_input_stops_before_later_global_handlers() {
    let mut application = application_with_ordered_input_components();

    assert_eq!(
        application.update(UserInput::Key(Key::Char('q'))),
        [Action::Terminal(TerminalAction::Quit)]
    );
}

#[test]
fn focused_input_runs_before_global_input() {
    let mut application = application_with_ordered_input_components();

    assert_eq!(
        application.update(UserInput::Key(Key::Char('x'))),
        [Action::Terminal(TerminalAction::Quit)]
    );
}

#[test]
fn completed_global_shortcut_returns_input_to_the_focused_component() {
    let mut application = application();
    application.mount_input_component_for_test(|_| FocusedComponent, true);

    assert!(
        application
            .update(UserInput::Key(Key::Char('r')))
            .is_empty()
    );
    assert!(
        application
            .update(UserInput::Key(Key::Char('f')))
            .is_empty()
    );
    assert_eq!(
        application.update(UserInput::Key(Key::Char('x'))),
        [Action::Terminal(TerminalAction::Quit)]
    );
}

#[test]
fn unmatched_focused_input_falls_back_to_global_handlers() {
    let mut application = application_with_ordered_input_components();

    assert_eq!(
        application.update(UserInput::Key(Key::Char('~'))),
        [Action::Lsp(LspAction::Restart)]
    );
}

#[test]
fn location_click_uses_the_visible_row_after_pointer_scrolling() {
    let mut application = application();
    application.update(UserInput::Resize {
        width: 80,
        height: 8,
    });
    publish_repository(
        &mut application,
        ReviewCheckpoint::new("change", "snapshot"),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    application.publish(LspEvent::Locations {
        toast_id: ToastId::generate(),
        operation: Operation::References,
        snapshot_id: "snapshot".to_owned(),
        locations: (0..8)
            .map(|line| SourceLocation {
                path: PathBuf::from("src/other.rs"),
                line,
                byte_column: 0,
                end_line: line,
                end_byte_column: 1,
            })
            .collect(),
    });

    application.update(UserInput::MouseScroll {
        column: 2,
        row: 3,
        delta: 5,
    });
    let actions = application.update(UserInput::MouseClick { column: 2, row: 2 });

    assert!(
        matches!(
            actions.as_slice(),
            [Action::Document(DocumentAction::Load(DocumentLoad::Source { location, .. }))] if location.line == 2
        ),
        "unexpected actions: {actions:?}"
    );
}

#[test]
fn revision_navigation_restores_the_file_after_the_new_files_arrive() {
    let mut application = application();
    publish_repository(
        &mut application,
        ReviewCheckpoint::new("old", "old-snapshot"),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );

    application.update(UserInput::Key(Key::Char('[')));
    application.update(UserInput::Key(Key::Char('v')));
    assert_eq!(
        application.publish(RevisionCandidatesLoaded {
            direction: RevisionDirection::Parents,
            result: Ok(vec![RevisionCandidate {
                change_id: ChangeId::from("new".to_owned()),
                short_change_id: "new".to_owned(),
                description: String::new(),
            }]),
        }),
        [Action::Repository(RepositoryAction::EditRevision {
            change_id: ChangeId::from("new".to_owned()),
        })]
    );

    assert_eq!(
        publish_repository(
            &mut application,
            ReviewCheckpoint::new("new", "new-snapshot"),
            String::new(),
            vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)]
        ),
        [
            Action::Document(DocumentAction::Load(DocumentLoad::Diff {
                review_checkpoint: ReviewCheckpoint::new("new", "new-snapshot"),
                path: "src/lib.rs".to_owned(),
            })),
            Action::Thread(review_threads::ThreadCommand::Load("new".into()))
        ]
    );
}

#[test]
fn failed_history_revision_edit_keeps_the_previous_location_available() {
    let mut application = application();
    publish_repository(
        &mut application,
        ReviewCheckpoint::new("old", "old-snapshot"),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    application.update(UserInput::Key(Key::Char('[')));
    application.update(UserInput::Key(Key::Char('v')));
    application.publish(RevisionCandidatesLoaded {
        direction: RevisionDirection::Parents,
        result: Ok(vec![RevisionCandidate {
            change_id: ChangeId::from("new".to_owned()),
            short_change_id: "new".to_owned(),
            description: String::new(),
        }]),
    });
    publish_repository(
        &mut application,
        ReviewCheckpoint::new("new", "new-snapshot"),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    application.update(UserInput::Key(Key::Tab));

    let expected = [Action::Repository(RepositoryAction::EditRevision {
        change_id: ChangeId::from("old".to_owned()),
    })];
    assert_eq!(
        application.update(UserInput::Key(Key::PreviousLocation)),
        expected
    );
    application.publish(RevisionEditFailed { message: None });
    assert_eq!(
        application.update(UserInput::Key(Key::PreviousLocation)),
        expected
    );
}

fn rendered_text_position(
    application: &ReviewApplication,
    text: &str,
    width: u16,
    height: u16,
) -> Option<(u16, u16)> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).ok()?;
    terminal
        .draw(|frame| frame.render_widget(application.frame(), frame.area()))
        .ok()?;
    let buffer = terminal.backend().buffer();
    (0..height).find_map(|row| {
        let line = (0..width)
            .map(|column| buffer[(column, row)].symbol())
            .collect::<String>();
        let byte = line.find(text)?;
        let mut offset = 0;
        (0..width).find_map(|column| {
            let matched = offset == byte;
            offset += buffer[(column, row)].symbol().len();
            matched.then_some((column, row))
        })
    })
}

fn rendered_application(application: &ReviewApplication) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(application.frame(), frame.area()))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}
