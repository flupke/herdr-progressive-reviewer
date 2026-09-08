use std::path::PathBuf;
use std::time::Instant;
use ui_events::{
    AnimationTick, FileSummary, RepositoryFilesChanged, RepositoryMetadataChanged,
    ReviewGuideChanged, RevisionCandidatesLoaded, RevisionEditFailed, RevisionHistoryLoadId,
    RevisionHistoryLoaded, SourceContentLoaded, ToastExpirationTick,
};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use review_guide::{
    GuideItem, GuideItemStatus, GuideLineRange, GuideScope, GuideTarget, ReviewCheckpoint,
};
use review_lsp::{Event as LspEvent, Operation, SourceLocation};
use review_repository::diff::DiffRow;
use review_repository::repository::{
    ChangeId, RevisionCandidate, RevisionDirection, RevisionHistoryLine,
};
use review_state::ReviewStatus;
use review_types::ReviewUnit;
use toasts::ToastId;

use super::*;
use crate::Key;
use component_core::{
    AnyInput, Component, ComponentSubscriptions, InputMatcher, InputResolution, InputScope,
};

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
        vec![Action::RestartLsp]
    }
}

struct FocusedComponent;

impl Component<Action> for FocusedComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe_input(
            InputScope::Focused,
            MatchingKeys(&[Key::Quit, Key::Char('x')]),
            Self::input,
        );
    }
}

impl FocusedComponent {
    #[allow(clippy::unused_self)]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn input(&mut self, _input: Key) -> Vec<Action> {
        vec![Action::Quit]
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
        vec![Action::RestartLsp]
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
    assert!(
        !initial_actions
            .iter()
            .any(|action| matches!(action, Action::LoadDiff { .. }))
    );

    let actions = application.update(UserInput::Key(Key::Space));

    assert_eq!(
        actions,
        vec![
            Action::SetReviewed {
                path: "src/lib.rs".to_owned(),
                reviewed: false,
            },
            Action::LoadDiff {
                review_checkpoint,
                path: "src/lib.rs".to_owned(),
            },
        ]
    );
}

#[test]
fn space_marks_the_selected_file_reviewed_from_either_pane() {
    for focus_diff in [false, true] {
        for key in [Key::Space, Key::Char(' ')] {
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
                application.update(UserInput::Key(key)),
                vec![Action::SetReviewed {
                    path: "src/lib.rs".to_owned(),
                    reviewed: true,
                }],
            );
        }
    }
}

#[test]
fn guide_prefixes_do_not_block_revision_navigation() {
    let mut parent_application = application();
    publish_repository(
        &mut parent_application,
        ReviewCheckpoint::new(ReviewUnit::from("change"), "commit".to_owned()),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    assert!(
        parent_application
            .update(UserInput::Key(Key::Tab))
            .is_empty()
    );

    assert!(
        parent_application
            .update(UserInput::Key(Key::Char('[')))
            .is_empty()
    );
    assert_eq!(
        parent_application.update(UserInput::Key(Key::Char('v'))),
        vec![Action::LoadRevisionCandidates(RevisionDirection::Parents)]
    );

    let mut child_application = application();
    publish_repository(
        &mut child_application,
        ReviewCheckpoint::new(ReviewUnit::from("change"), "commit".to_owned()),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    assert!(
        child_application
            .update(UserInput::Key(Key::Tab))
            .is_empty()
    );
    assert!(
        child_application
            .update(UserInput::Key(Key::Char(']')))
            .is_empty()
    );
    assert_eq!(
        child_application.update(UserInput::Key(Key::Char('v'))),
        vec![Action::LoadRevisionCandidates(RevisionDirection::Children)]
    );
}

#[test]
fn completed_guide_shortcuts_do_not_leave_stale_revision_prefixes() {
    let mut application = application();
    publish_repository(
        &mut application,
        ReviewCheckpoint::new("change", "commit"),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );

    assert!(
        application
            .update(UserInput::Key(Key::Char('r')))
            .is_empty()
    );
    assert_eq!(
        application.update(UserInput::Key(Key::Char('f'))),
        [Action::GenerateReviewGuide {
            scope: GuideScope::File {
                path: "src/lib.rs".to_owned(),
            },
        }]
    );
    assert!(
        application
            .update(UserInput::Key(Key::Char('[')))
            .is_empty()
    );
    assert_eq!(
        application.update(UserInput::Key(Key::Char('v'))),
        [Action::LoadRevisionCandidates(RevisionDirection::Parents)]
    );
}

#[test]
fn completed_guide_navigation_does_not_block_the_next_revision_shortcut() {
    let mut application = application();
    publish_repository(
        &mut application,
        ReviewCheckpoint::new("change", "commit"),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );

    assert!(
        application
            .update(UserInput::Key(Key::Char('[')))
            .is_empty()
    );
    assert!(
        application
            .update(UserInput::Key(Key::Char('r')))
            .is_empty()
    );
    assert!(
        application
            .update(UserInput::Key(Key::Char(']')))
            .is_empty()
    );
    assert_eq!(
        application.update(UserInput::Key(Key::Char('v'))),
        [Action::LoadRevisionCandidates(RevisionDirection::Children)]
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
        vec![Action::LoadRevisionCandidates(RevisionDirection::Parents)]
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
        [Action::LoadRevisionHistory {
            load_id: RevisionHistoryLoadId::new(0),
        }]
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
            short_change_id: Some("change".to_owned()),
            change_id: Some(ChangeId::from("change".to_owned())),
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
            short_change_id: Some("change".to_owned()),
            change_id: Some(ChangeId::from("change".to_owned())),
            is_current: true,
            is_immutable: false,
        }]),
    });
    assert!(rendered_application(&application).contains("Select revision"));

    application.update(UserInput::MouseClick {
        column: 0,
        row: 0,
        insert_path: false,
    });

    assert!(!rendered_application(&application).contains("Select revision"));
}

#[test]
fn clicking_outside_the_help_popup_closes_it() {
    let mut application = application();
    application.update(UserInput::Key(Key::Char('?')));
    assert!(rendered_application(&application).contains("Keyboard shortcuts"));

    application.update(UserInput::MouseClick {
        column: 0,
        row: 0,
        insert_path: false,
    });

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

#[test]
fn guide_navigation_clears_the_shared_shortcut_prefix() {
    for prefix in ['[', ']'] {
        let mut review_application = application();
        publish_repository(
            &mut review_application,
            ReviewCheckpoint::new(ReviewUnit::from("change"), "commit".to_owned()),
            String::new(),
            vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
        );
        assert!(
            review_application
                .update(UserInput::Key(Key::Tab))
                .is_empty()
        );

        assert!(
            review_application
                .update(UserInput::Key(Key::Char(prefix)))
                .is_empty()
        );
        assert!(
            review_application
                .update(UserInput::Key(Key::Char('r')))
                .is_empty()
        );
        assert!(
            review_application
                .update(UserInput::Key(Key::Char('g')))
                .is_empty()
        );
    }
}

fn application_with_ordered_input_components() -> ReviewApplication {
    let mut application = application();
    application.mount_input_component_for_test(
        |_| SelectiveGlobalComponent,
        Rect::default(),
        false,
    );
    application.mount_input_component_for_test(|_| FocusedComponent, Rect::default(), true);
    application.mount_input_component_for_test(|_| GlobalComponent, Rect::default(), false);
    application
}

#[test]
fn unhandled_files_input_reaches_the_global_application_controller() {
    let mut application = application();

    assert_eq!(
        application.update(UserInput::Key(Key::Quit)),
        [Action::Quit]
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
    assert!(rendered.contains("Files"));
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

    application.update(UserInput::MouseClick {
        column: 3,
        row: 3,
        insert_path: false,
    });
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
        [Action::LoadDiff {
            review_checkpoint: ReviewCheckpoint::new("change", "commit"),
            path: "first.rs".to_owned(),
        }]
    );

    assert_eq!(application.update(UserInput::Key(Key::Down)), []);
    assert_eq!(
        publish_tick(&mut application, Instant::now()),
        [Action::LoadDiff {
            review_checkpoint: ReviewCheckpoint::new("change", "commit"),
            path: "second.rs".to_owned(),
        }]
    );
}

#[test]
fn control_clicking_a_file_inserts_its_path() {
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

    assert_eq!(
        application.update(UserInput::MouseControlClick { column: 1, row: 2 }),
        [Action::Output {
            text: "lib.rs".to_owned(),
        }]
    );
}

#[test]
fn matched_focused_input_stops_before_later_global_handlers() {
    let mut application = application_with_ordered_input_components();

    assert_eq!(
        application.update(UserInput::Key(Key::Quit)),
        [Action::Quit]
    );
}

#[test]
fn focused_input_runs_before_global_input() {
    let mut application = application_with_ordered_input_components();

    assert_eq!(
        application.update(UserInput::Key(Key::Char('x'))),
        [Action::Quit]
    );
}

#[test]
fn completed_global_shortcut_returns_input_to_the_focused_component() {
    let mut application = application();
    application.mount_input_component_for_test(|_| FocusedComponent, Rect::default(), true);

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
        [Action::Quit]
    );
}

#[test]
fn unmatched_focused_input_falls_back_to_global_handlers() {
    let mut application = application_with_ordered_input_components();

    assert_eq!(
        application.update(UserInput::Key(Key::Char('~'))),
        [Action::RestartLsp]
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
    let actions = application.update(UserInput::MouseClick {
        column: 2,
        row: 2,
        insert_path: false,
    });

    assert!(
        matches!(
            actions.as_slice(),
            [Action::LoadSource { location, .. }] if location.line == 2
        ),
        "unexpected actions: {actions:?}"
    );
}

#[test]
fn diff_pointer_selection_accounts_for_rendered_guide_rows() {
    let theme = Theme::default();
    let mut application = ReviewApplication::new(theme, Some(24), PathBuf::new());
    let review_checkpoint = ReviewCheckpoint::new("change", "checkpoint");
    application.update(UserInput::Resize {
        width: 80,
        height: 16,
    });
    publish_repository(
        &mut application,
        review_checkpoint.clone(),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    application.publish(ui_events::DiffContentLoaded {
        review_checkpoint: review_checkpoint.clone(),
        path: "src/lib.rs".to_owned(),
        rows: vec![
            DiffRow::Add {
                new_line: 1,
                text: "+first_pointer_target".to_owned(),
            },
            DiffRow::Add {
                new_line: 2,
                text: "+second_pointer_target".to_owned(),
            },
            DiffRow::Add {
                new_line: 3,
                text: "+third_pointer_target".to_owned(),
            },
        ],
        old_content: None,
        new_content: None,
    });
    application.publish(ReviewGuideChanged {
        review_checkpoint,
        items: vec![GuideItem {
            target: GuideTarget::Lines {
                path: "src/lib.rs".to_owned(),
                old: None,
                new: Some(GuideLineRange {
                    first_line: 1,
                    last_line: 1,
                }),
            },
            text: "Guide text that inserts visual rows before the source.".to_owned(),
            status: GuideItemStatus::Matched,
        }],
    });
    application.update(UserInput::Key(Key::Tab));

    let (word_column, row) = rendered_text_position(&application, "third_pointer_target", 80, 16)
        .expect("the third source row must be visible");
    let column = word_column + 5;
    application.update(UserInput::MouseClick {
        column,
        row,
        insert_path: false,
    });

    let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(application.frame(), frame.area()))
        .unwrap();
    assert_eq!(
        terminal.backend().buffer()[(column, row)].bg,
        theme.palette.cursor
    );
    assert!(
        terminal.backend().buffer()[(column, row)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
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
        [Action::EditRevision {
            change_id: ChangeId::from("new".to_owned()),
        }]
    );

    assert_eq!(
        publish_repository(
            &mut application,
            ReviewCheckpoint::new("new", "new-snapshot"),
            String::new(),
            vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)]
        ),
        [Action::LoadDiff {
            review_checkpoint: ReviewCheckpoint::new("new", "new-snapshot"),
            path: "src/lib.rs".to_owned(),
        }]
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

    let expected = [Action::EditRevision {
        change_id: ChangeId::from("old".to_owned()),
    }];
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
        let column = u16::try_from(line.find(text)?).ok()?;
        Some((column, row))
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
