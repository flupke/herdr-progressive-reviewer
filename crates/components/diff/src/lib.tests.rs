use component_core::{
    Component, ComponentEventBus, ComponentSubscriptions, ComponentTarget, DispatchResult,
    EventEnvelope,
};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use review_guide::ReviewCheckpoint;
use review_repository::{
    diff::{DiffRow, NoticeKind},
    repository::DiffStatistics,
};
use review_state::{ReviewState, ReviewStatus};
use two_face::theme::EmbeddedThemeName;
use ui_events::{FileSummary, PointerPosition};
use ui_theme::Theme;

use super::*;

#[test]
fn loaded_content_publishes_its_guide_viewport() {
    let (mut registry, reviewable_files, _) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish_envelope(EventEnvelope::new(FileSelected {
            path: "src/lib.rs".to_owned(),
        }))
        .expect("file selection must dispatch");

    let results = registry
        .publish_envelope(EventEnvelope::new(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: changed_rows(),
            old_content: None,
            new_content: None,
        }))
        .expect("loaded diff must dispatch");

    assert_eq!(output_texts(results), vec!["src/lib.rs:0:1"]);
}

#[test]
fn rapid_file_selection_loads_only_the_last_selected_diff_on_the_next_tick() {
    let (mut registry, reviewable_files, _) = registry_with_observer();
    reviewable_files.replace(
        [
            "first.rs".to_owned(),
            "second.rs".to_owned(),
            "third.rs".to_owned(),
        ]
        .into(),
    );
    registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            files: ["first.rs", "second.rs", "third.rs"]
                .into_iter()
                .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
                .collect(),
        }))
        .expect("repository must dispatch");
    registry
        .publish_envelope(EventEnvelope::new(FileSelected {
            path: "first.rs".to_owned(),
        }))
        .expect("initial selection must dispatch");
    registry
        .publish_envelope(EventEnvelope::new(FileSelected {
            path: "second.rs".to_owned(),
        }))
        .expect("second selection must dispatch");
    registry
        .publish_envelope(EventEnvelope::new(FileSelected {
            path: "third.rs".to_owned(),
        }))
        .expect("third selection must dispatch");

    let actions = registry
        .publish_envelope(EventEnvelope::new(AnimationTick))
        .expect("tick must dispatch")
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();

    assert_eq!(
        actions,
        vec![Action::LoadDiff {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "third.rs".to_owned(),
        }]
    );
}

#[test]
fn loaded_content_from_another_review_unit_does_not_publish_a_viewport() {
    let (mut registry, reviewable_files, _) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    publish_repository(&mut registry, "current");

    let results = registry
        .publish_envelope(EventEnvelope::new(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("other-change", "current"),
            path: "src/lib.rs".to_owned(),
            rows: changed_rows(),
            old_content: None,
            new_content: None,
        }))
        .expect("stale loaded diff must dispatch");

    assert!(output_texts(results).is_empty());
}

#[test]
fn unreviewing_a_checkpointed_file_reloads_its_full_diff() {
    let (mut registry, reviewable_files, _) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .expect("file selection must dispatch");
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: changed_rows(),
            old_content: None,
            new_content: None,
        })
        .expect("checkpoint diff must load");

    let actions = registry
        .publish(ReviewStateSaved {
            review_unit: "change".into(),
            path: "src/lib.rs".to_owned(),
            result: Ok(ReviewState::unreviewed(DiffStatistics::default(), None)),
        })
        .expect("review state must dispatch")
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();

    assert_eq!(
        actions,
        vec![Action::LoadDiff {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
        }]
    );
}

#[test]
fn unreviewing_while_checkpoint_diff_loads_defers_the_full_diff_load() {
    let (mut registry, reviewable_files, _) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .expect("file selection must dispatch");

    let actions = registry
        .publish(ReviewStateSaved {
            review_unit: "change".into(),
            path: "src/lib.rs".to_owned(),
            result: Ok(ReviewState::unreviewed(DiffStatistics::default(), None)),
        })
        .expect("review state must dispatch")
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();
    assert!(actions.is_empty());

    let load_actions = registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: changed_rows(),
            old_content: None,
            new_content: None,
        })
        .expect("checkpoint diff must load")
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .filter(|action| matches!(action, Action::LoadDiff { .. }))
        .collect::<Vec<_>>();

    assert_eq!(
        load_actions,
        vec![Action::LoadDiff {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
        }]
    );
}

#[test]
fn stale_load_failure_starts_full_load_then_full_load_failure_waits_for_refresh() {
    let (mut registry, reviewable_files, _) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .expect("file selection must dispatch");
    registry
        .publish(ReviewStateSaved {
            review_unit: "change".into(),
            path: "src/lib.rs".to_owned(),
            result: Ok(ReviewState::unreviewed(DiffStatistics::default(), None)),
        })
        .expect("review state must dispatch");

    let stale_failure_actions = registry
        .publish(DiffContentLoadFailed {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
        })
        .expect("load failure must dispatch")
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .filter(|action| matches!(action, Action::LoadDiff { .. }))
        .collect::<Vec<_>>();
    assert_eq!(
        stale_failure_actions,
        vec![Action::LoadDiff {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
        }]
    );

    let replacement_failure_actions = registry
        .publish(DiffContentLoadFailed {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
        })
        .expect("replacement load failure must dispatch")
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .filter(|action| matches!(action, Action::LoadDiff { .. }))
        .collect::<Vec<_>>();
    assert!(replacement_failure_actions.is_empty());

    let refresh_actions = registry
        .publish(ReviewableFilesChanged)
        .expect("reviewable-file refresh must dispatch")
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .filter(|action| matches!(action, Action::LoadDiff { .. }))
        .collect::<Vec<_>>();
    assert_eq!(
        refresh_actions,
        vec![Action::LoadDiff {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
        }]
    );
}

#[test]
fn source_load_failures_only_report_for_the_current_snapshot() {
    let (mut registry, reviewable_files, _) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    registry.mount(|_| ToastObserver);
    publish_repository(&mut registry, "current");

    let stale = registry
        .publish(SourceContentLoadFailed {
            snapshot_id: "stale".to_owned(),
            message: "stale failure".to_owned(),
        })
        .unwrap();
    let current = registry
        .publish(SourceContentLoadFailed {
            snapshot_id: "current".to_owned(),
            message: "current failure".to_owned(),
        })
        .unwrap();

    assert!(output_texts(stale).is_empty());
    assert_eq!(output_texts(current), ["toast:current failure"]);
}

#[test]
fn source_preview_replaces_the_diff_until_the_location_list_closes() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: vec![DiffRow::Add {
                new_line: 1,
                text: "+selected content".to_owned(),
            }],
            old_content: None,
            new_content: None,
        })
        .unwrap();
    let location = review_lsp::SourceLocation {
        path: PathBuf::from("/outside/preview.rs"),
        line: 0,
        byte_column: 0,
        end_line: 0,
        end_byte_column: 2,
    };
    registry
        .publish(SourceLocationPreviewRequested {
            location: location.clone(),
        })
        .unwrap();
    registry
        .publish(SourceContentLoaded {
            snapshot_id: "checkpoint".to_owned(),
            location,
            content: b"fn preview_content() {}\n".to_vec(),
            mode: ui_actions::SourceLoadMode::Preview,
        })
        .unwrap();

    assert!(rendered_diff(&registry, diff_target).contains("preview_content"));

    registry
        .publish(LocationListVisibilityChanged { visible: false })
        .unwrap();
    assert!(rendered_diff(&registry, diff_target).contains("selected content"));
}

#[test]
fn accepted_location_is_centered_after_its_preview_started_the_diff_load() {
    let mut registry = ComponentEventBus::new();
    let reviewable_files = ReviewableFiles::default();
    reviewable_files.replace(["src/origin.rs".to_owned(), "src/definition.rs".to_owned()].into());
    let diff_target = registry.mount(|events| {
        DiffComponent::new(
            events,
            reviewable_files,
            SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White),
            PathBuf::from("/repo"),
            Theme::default().palette,
        )
    });
    registry.mount(|_| FullLocationObserver);
    registry
        .publish(DiffViewportChanged {
            width: 80,
            height: 12,
        })
        .unwrap();
    registry
        .publish(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            files: ["src/origin.rs", "src/definition.rs"]
                .into_iter()
                .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
                .collect(),
        })
        .unwrap();
    registry
        .publish(FileSelected {
            path: "src/origin.rs".to_owned(),
        })
        .unwrap();
    let location = review_lsp::SourceLocation {
        path: PathBuf::from("/repo/src/definition.rs"),
        line: 9,
        byte_column: 4,
        end_line: 9,
        end_byte_column: 10,
    };

    let preview_actions = registry
        .publish(SourceLocationPreviewRequested {
            location: location.clone(),
        })
        .unwrap()
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();
    assert!(
        preview_actions.iter().any(
            |action| matches!(action, Action::LoadDiff { path, .. } if path == "src/definition.rs")
        ),
        "unexpected actions: {preview_actions:?}"
    );
    let accepted_actions = registry
        .publish(SourceLocationAccepted { location })
        .unwrap()
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();
    assert!(
        !accepted_actions
            .iter()
            .any(|action| matches!(action, Action::LoadDiff { .. })),
        "the diff load is already active: {accepted_actions:?}"
    );

    let loaded = registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/definition.rs".to_owned(),
            rows: context_rows(20),
            old_content: None,
            new_content: None,
        })
        .unwrap();

    assert!(output_texts(loaded).contains(&"location:src/definition.rs:9:4".to_owned()));
    assert!(
        first_rendered_source_line(&registry, diff_target).contains("line 4"),
        "the LSP target must be centered when space permits"
    );
}

#[test]
fn completed_background_load_clears_its_pending_center() {
    let mut registry = ComponentEventBus::new();
    let reviewable_files = ReviewableFiles::default();
    reviewable_files.replace(["src/origin.rs".to_owned(), "src/definition.rs".to_owned()].into());
    let diff_target = registry.mount(|events| {
        DiffComponent::new(
            events,
            reviewable_files,
            SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White),
            PathBuf::from("/repo"),
            Theme::default().palette,
        )
    });
    registry
        .publish(DiffViewportChanged {
            width: 80,
            height: 12,
        })
        .unwrap();
    registry
        .publish(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            files: ["src/origin.rs", "src/definition.rs"]
                .into_iter()
                .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
                .collect(),
        })
        .unwrap();
    registry
        .publish(FileSelected {
            path: "src/origin.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(SourceLocationAccepted {
            location: review_lsp::SourceLocation {
                path: PathBuf::from("/repo/src/definition.rs"),
                line: 9,
                byte_column: 4,
                end_line: 9,
                end_byte_column: 10,
            },
        })
        .unwrap();
    registry
        .publish(FileSelected {
            path: "src/origin.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/definition.rs".to_owned(),
            rows: context_rows(20),
            old_content: None,
            new_content: None,
        })
        .unwrap();
    registry
        .publish(FileSelected {
            path: "src/definition.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/definition.rs".to_owned(),
            rows: context_rows(20),
            old_content: None,
            new_content: None,
        })
        .unwrap();

    assert!(
        first_rendered_source_line(&registry, diff_target).contains("line 1"),
        "a completed background jump must not recenter a later refresh"
    );
}

#[test]
fn location_history_jumps_use_the_same_centering_rule_as_first_and_last() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffViewportChanged {
            width: 80,
            height: 12,
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: context_rows(30),
            old_content: None,
            new_content: None,
        })
        .unwrap();

    dispatch_key(&mut registry, diff_target, Key::Char('G'));
    assert!(
        first_rendered_source_line(&registry, diff_target).contains("line 19"),
        "the last row must use the final full viewport"
    );

    dispatch_key(&mut registry, diff_target, Key::PreviousLocation);
    assert!(
        first_rendered_source_line(&registry, diff_target).contains("line 1"),
        "the previous location is too close to the start to center"
    );

    dispatch_key(&mut registry, diff_target, Key::NextLocation);
    assert!(
        first_rendered_source_line(&registry, diff_target).contains("line 19"),
        "the next location must restore the centered last-row viewport"
    );
}

#[test]
fn modified_hunk_shortcuts_wrap_and_center_the_target() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    let other_target = registry.mount(|_| ViewportObserver);
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffViewportChanged {
            width: 80,
            height: 6,
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: two_hunk_rows(),
            old_content: None,
            new_content: None,
        })
        .unwrap();

    let component = registry.get::<DiffComponent>(diff_target).unwrap();
    let document = component.selected_document().unwrap();
    assert_eq!(document.document.diff.modified_hunk_rows(), vec![0, 7]);

    dispatch_global_shortcut(&mut registry, other_target, ']', 'h');

    let component = registry.get::<DiffComponent>(diff_target).unwrap();
    let document = component.selected_document().unwrap();
    assert_eq!(document.document.cursor, 7);
    assert_eq!(document.document.scroll, 3);

    dispatch_global_shortcut(&mut registry, other_target, '[', 'h');

    let component = registry.get::<DiffComponent>(diff_target).unwrap();
    let document = component.selected_document().unwrap();
    assert_eq!(document.document.cursor, 0);
    assert_eq!(document.document.scroll, 0);
}

#[test]
fn refreshed_checkpoint_restarts_an_in_flight_definition_load() {
    let mut registry = ComponentEventBus::new();
    let reviewable_files = ReviewableFiles::default();
    reviewable_files.replace(["src/origin.rs".to_owned(), "src/definition.rs".to_owned()].into());
    registry.mount(|events| {
        DiffComponent::new(
            events,
            reviewable_files,
            SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White),
            PathBuf::from("/repo"),
            Theme::default().palette,
        )
    });
    registry.mount(|_| FullLocationObserver);
    let repository_event = |checkpoint| RepositoryFilesChanged {
        review_checkpoint: ReviewCheckpoint::new("change", checkpoint),
        files: ["src/origin.rs", "src/definition.rs"]
            .into_iter()
            .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
            .collect(),
    };
    registry.publish(repository_event("first")).unwrap();
    registry
        .publish(FileSelected {
            path: "src/origin.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(SourceLocationAccepted {
            location: review_lsp::SourceLocation {
                path: PathBuf::from("/repo/src/definition.rs"),
                line: 9,
                byte_column: 4,
                end_line: 9,
                end_byte_column: 10,
            },
        })
        .unwrap();

    let refreshed = registry
        .publish(repository_event("second"))
        .unwrap()
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();
    assert!(refreshed.iter().any(|action| matches!(
        action,
        Action::LoadDiff {
            review_checkpoint,
            path,
        } if review_checkpoint.checkpoint == "second" && path == "src/definition.rs"
    )));

    let loaded = registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "second"),
            path: "src/definition.rs".to_owned(),
            rows: context_rows(20),
            old_content: None,
            new_content: None,
        })
        .unwrap();

    assert!(output_texts(loaded).contains(&"location:src/definition.rs:9:4".to_owned()));
}

#[test]
fn repository_refresh_preserves_loaded_content_and_cursor_output() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    publish_repository(&mut registry, "first");
    registry
        .publish_envelope(EventEnvelope::new(FileSelected {
            path: "src/lib.rs".to_owned(),
        }))
        .expect("file selection must dispatch");
    let mut rows = changed_rows();
    rows.push(DiffRow::Add {
        new_line: 2,
        text: "+second".to_owned(),
    });
    registry
        .publish_envelope(EventEnvelope::new(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "first"),
            path: "src/lib.rs".to_owned(),
            rows,
            old_content: None,
            new_content: None,
        }))
        .expect("loaded diff must dispatch");
    let moved = registry
        .dispatch_hovered_input(
            &EventEnvelope::new(pointer_input(
                PointerInputKind::Click { insert: false },
                2,
                4,
            )),
            diff_target,
        )
        .expect("pointer input must dispatch")
        .into_results();
    assert_eq!(output_texts(moved), vec!["src/lib.rs:1:2"]);

    let refreshed = registry
        .publish_envelope(EventEnvelope::new(repository_event("second")))
        .expect("repository refresh must dispatch");

    assert_eq!(output_texts(refreshed), vec!["src/lib.rs:1:2"]);
}

#[test]
fn clicking_an_unmodified_section_expands_it() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: vec![
                DiffRow::Hunk {
                    old_start: 1,
                    old_count: 1,
                    new_start: 1,
                    new_count: 1,
                },
                DiffRow::Context {
                    old_line: 1,
                    new_line: 1,
                    text: " first".to_owned(),
                },
                DiffRow::Hunk {
                    old_start: 5,
                    old_count: 1,
                    new_start: 5,
                    new_count: 1,
                },
                DiffRow::Context {
                    old_line: 5,
                    new_line: 5,
                    text: " fifth".to_owned(),
                },
            ],
            old_content: Some(b"first\nsecond\nthird\nfourth\nfifth\n".to_vec()),
            new_content: Some(b"first\nsecond\nthird\nfourth\nfifth\n".to_vec()),
        })
        .unwrap();
    let collapsed_lines = rendered_diff_lines(&registry, diff_target);
    let gap_row = collapsed_lines
        .iter()
        .position(|line| line.contains("3 unmodified lines"))
        .expect("the diff must contain a collapsed section");

    registry
        .dispatch_hovered_input(
            &EventEnvelope::new(pointer_input(
                PointerInputKind::Click { insert: false },
                u16::try_from(gap_row).unwrap(),
                4,
            )),
            diff_target,
        )
        .unwrap();

    let expanded = rendered_diff(&registry, diff_target);
    assert!(!expanded.contains("unmodified lines"));
    assert!(expanded.contains("second"));
    assert!(expanded.contains("fourth"));
}

#[test]
fn dragging_source_rows_still_outputs_the_selected_diff() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();
    let rows = vec![
        DiffRow::FileHeader {
            old_path: None,
            new_path: None,
            text: "diff --git a/src/lib.rs b/src/lib.rs".to_owned(),
        },
        DiffRow::Meta {
            text: "--- a/src/lib.rs".to_owned(),
        },
        DiffRow::Meta {
            text: "+++ b/src/lib.rs".to_owned(),
        },
        DiffRow::Hunk {
            old_start: 1,
            old_count: 2,
            new_start: 1,
            new_count: 2,
        },
        DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: " fn run() {".to_owned(),
        },
        DiffRow::Delete {
            old_line: 2,
            text: "-    old();".to_owned(),
        },
        DiffRow::Add {
            new_line: 2,
            text: "+    new();".to_owned(),
        },
    ];
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows,
            old_content: None,
            new_content: None,
        })
        .unwrap();
    let rendered_lines = rendered_diff_lines(&registry, diff_target);
    let selection_start_row = rendered_lines
        .iter()
        .position(|line| line.contains("fn run"))
        .expect("the selection start row must be visible");
    let selection_end_row = rendered_lines
        .iter()
        .position(|line| line.contains("new();"))
        .expect("the selection end row must be visible");

    for (kind, row) in [
        (
            PointerInputKind::Click { insert: false },
            selection_start_row,
        ),
        (PointerInputKind::Drag, selection_end_row),
    ] {
        registry
            .dispatch_hovered_input(
                &EventEnvelope::new(pointer_input(kind, u16::try_from(row).unwrap(), 4)),
                diff_target,
            )
            .unwrap();
    }
    let released = registry
        .dispatch_hovered_input(
            &EventEnvelope::new(pointer_input(PointerInputKind::Release, 0, 0)),
            diff_target,
        )
        .unwrap()
        .into_results();

    let output = output_texts(released);
    assert!(
        output
            .iter()
            .any(|text| text.contains("-    old();") && text.contains("+    new();")),
        "unexpected pointer selection output: {output:?}"
    );
}

#[test]
fn search_input_publishes_matching_file_decorations() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(
        [
            "src/first.rs".to_owned(),
            "src/second.rs".to_owned(),
            "src/unloaded.rs".to_owned(),
        ]
        .into(),
    );
    registry.mount(|_| DecorationObserver);
    registry
        .publish_envelope(EventEnvelope::new(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            files: ["src/first.rs", "src/second.rs", "src/unloaded.rs"]
                .into_iter()
                .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
                .collect(),
        }))
        .expect("repository must dispatch");
    registry
        .publish_envelope(EventEnvelope::new(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/first.rs".to_owned(),
            rows: changed_rows(),
            old_content: None,
            new_content: None,
        }))
        .expect("loaded diff must dispatch");
    let mut second_rows = changed_rows();
    second_rows.push(DiffRow::Notice {
        kind: NoticeKind::Unsupported,
        text: "unsupported".to_owned(),
    });
    registry
        .publish_envelope(EventEnvelope::new(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/second.rs".to_owned(),
            rows: second_rows,
            old_content: None,
            new_content: None,
        }))
        .expect("second loaded diff must dispatch");
    registry
        .publish_envelope(EventEnvelope::new(FileSelected {
            path: "src/first.rs".to_owned(),
        }))
        .expect("file selection must dispatch");

    dispatch_key(&mut registry, diff_target, Key::Char('/'));
    let mut matching = Vec::new();
    for character in "changed".chars() {
        matching = dispatch_key(&mut registry, diff_target, Key::Char(character));
    }
    assert_eq!(
        decoration_texts(matching),
        vec!["notice:src/second.rs;search:src/first.rs,src/second.rs"]
    );

    let cleared = dispatch_key(&mut registry, diff_target, Key::Escape);
    assert_eq!(
        decoration_texts(cleared),
        vec!["notice:src/second.rs;search:"]
    );
}

#[test]
fn beginning_a_search_requests_every_unloaded_diff() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(["src/first.rs".to_owned(), "src/second.rs".to_owned()].into());
    registry
        .publish(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            files: ["src/first.rs", "src/second.rs"]
                .into_iter()
                .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
                .collect(),
        })
        .unwrap();
    registry
        .publish(FileSelected {
            path: "src/first.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/first.rs".to_owned(),
            rows: changed_rows(),
            old_content: None,
            new_content: None,
        })
        .unwrap();

    let actions = dispatch_key(&mut registry, diff_target, Key::Char('/'))
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();

    assert_eq!(
        actions,
        vec![Action::LoadDiffs {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            paths: vec!["src/second.rs".to_owned()],
        }]
    );
}

#[test]
fn completed_search_moves_to_a_match_that_finishes_loading_later() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(["src/first.rs".to_owned(), "src/second.rs".to_owned()].into());
    registry.mount(|_| FullLocationObserver);
    registry
        .publish(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            files: ["src/first.rs", "src/second.rs"]
                .into_iter()
                .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
                .collect(),
        })
        .unwrap();
    registry
        .publish(FileSelected {
            path: "src/first.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/first.rs".to_owned(),
            rows: vec![DiffRow::Add {
                new_line: 1,
                text: "+no match".to_owned(),
            }],
            old_content: None,
            new_content: None,
        })
        .unwrap();

    dispatch_key(&mut registry, diff_target, Key::Char('/'));
    for character in "needle".chars() {
        dispatch_key(&mut registry, diff_target, Key::Char(character));
    }
    dispatch_key(&mut registry, diff_target, Key::Enter);

    let loaded = registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/second.rs".to_owned(),
            rows: vec![DiffRow::Add {
                new_line: 1,
                text: "+needle".to_owned(),
            }],
            old_content: None,
            new_content: None,
        })
        .unwrap();

    assert!(
        output_texts(loaded).contains(&"location:src/second.rs:0:0".to_owned()),
        "a completed search must include late background results"
    );
}

#[test]
fn repository_refresh_reloads_every_file_for_an_active_search() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(["src/first.rs".to_owned(), "src/second.rs".to_owned()].into());
    registry
        .publish(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "first-checkpoint"),
            files: ["src/first.rs", "src/second.rs"]
                .into_iter()
                .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
                .collect(),
        })
        .unwrap();
    for path in ["src/first.rs", "src/second.rs"] {
        registry
            .publish(DiffContentLoaded {
                review_checkpoint: ReviewCheckpoint::new("change", "first-checkpoint"),
                path: path.to_owned(),
                rows: vec![DiffRow::Add {
                    new_line: 1,
                    text: "+needle".to_owned(),
                }],
                old_content: None,
                new_content: None,
            })
            .unwrap();
    }
    registry
        .publish(FileSelected {
            path: "src/first.rs".to_owned(),
        })
        .unwrap();
    dispatch_key(&mut registry, diff_target, Key::Char('/'));
    for character in "needle".chars() {
        dispatch_key(&mut registry, diff_target, Key::Char(character));
    }
    dispatch_key(&mut registry, diff_target, Key::Enter);

    let actions = registry
        .publish(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "second-checkpoint"),
            files: ["src/first.rs", "src/second.rs"]
                .into_iter()
                .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
                .collect(),
        })
        .unwrap()
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();

    assert!(actions.contains(&Action::LoadDiffs {
        review_checkpoint: ReviewCheckpoint::new("change", "second-checkpoint"),
        paths: vec!["src/first.rs".to_owned(), "src/second.rs".to_owned()],
    }));
}

#[test]
fn search_moves_between_every_occurrence_in_repository_order() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(
        [
            "src/first.rs".to_owned(),
            "src/second.rs".to_owned(),
            "src/third.rs".to_owned(),
        ]
        .into(),
    );
    registry.mount(|_| FullLocationObserver);
    registry.mount(|_| SearchStatusObserver);
    registry
        .publish(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            files: ["src/first.rs", "src/second.rs", "src/third.rs"]
                .into_iter()
                .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
                .collect(),
        })
        .unwrap();
    for (path, text) in [
        ("src/first.rs", "+no match"),
        ("src/second.rs", "+needle one needle"),
        ("src/third.rs", "+needle"),
    ] {
        registry
            .publish(DiffContentLoaded {
                review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
                path: path.to_owned(),
                rows: vec![DiffRow::Add {
                    new_line: 1,
                    text: text.to_owned(),
                }],
                old_content: None,
                new_content: None,
            })
            .unwrap();
    }
    registry
        .publish(FileSelected {
            path: "src/first.rs".to_owned(),
        })
        .unwrap();

    dispatch_key(&mut registry, diff_target, Key::Char('/'));
    let mut live_results = Vec::new();
    for character in "needle".chars() {
        live_results = dispatch_key(&mut registry, diff_target, Key::Char(character));
    }
    let live_texts = output_texts(live_results);
    assert!(
        live_texts.contains(&"location:src/second.rs:0:0".to_owned()),
        "unexpected search output: {live_texts:?}"
    );

    dispatch_key(&mut registry, diff_target, Key::Enter);
    let second_match = output_texts(dispatch_key(&mut registry, diff_target, Key::Char('n')));
    assert!(second_match.contains(&"location:src/second.rs:0:11".to_owned()));
    assert!(second_match.contains(&"search:needle:2/3".to_owned()));

    let third_match = output_texts(dispatch_key(&mut registry, diff_target, Key::Char('n')));
    assert!(third_match.contains(&"location:src/third.rs:0:0".to_owned()));
    assert!(third_match.contains(&"search:needle:3/3".to_owned()));

    let wrapped_match = output_texts(dispatch_key(&mut registry, diff_target, Key::Char('n')));
    assert!(wrapped_match.contains(&"location:src/second.rs:0:0".to_owned()));
    assert!(wrapped_match.contains(&"search:needle:1/3".to_owned()));

    let previous_match = output_texts(dispatch_key(&mut registry, diff_target, Key::Char('p')));
    assert!(previous_match.contains(&"location:src/third.rs:0:0".to_owned()));
    assert!(previous_match.contains(&"search:needle:3/3".to_owned()));
}

#[test]
fn source_shortcuts_move_between_columns_and_word_starts() {
    let mut registry = ComponentEventBus::new();
    let reviewable_files = ReviewableFiles::default();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    let diff_target = registry.mount(|events| {
        DiffComponent::new(
            events,
            reviewable_files,
            SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White),
            PathBuf::new(),
            Theme::default().palette,
        )
    });
    registry.mount(|_| LocationObserver);
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: vec![DiffRow::Add {
                new_line: 1,
                text: "+first second".to_owned(),
            }],
            old_content: None,
            new_content: None,
        })
        .unwrap();

    let right = dispatch_key(&mut registry, diff_target, Key::Char('l'));
    let left = dispatch_key(&mut registry, diff_target, Key::Char('h'));
    let next = dispatch_key(&mut registry, diff_target, Key::Char('w'));
    let previous = dispatch_key(&mut registry, diff_target, Key::Char('b'));

    assert_eq!(output_texts(right), ["column:1"]);
    assert_eq!(output_texts(left), ["column:0"]);
    assert_eq!(output_texts(next), ["column:6"]);
    assert_eq!(output_texts(previous), ["column:0"]);
}

#[test]
fn source_shortcuts_follow_character_boundaries_and_line_ends() {
    let mut registry = ComponentEventBus::new();
    let reviewable_files = ReviewableFiles::default();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    let diff_target = registry.mount(|events| {
        DiffComponent::new(
            events,
            reviewable_files,
            SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White),
            PathBuf::new(),
            Theme::default().palette,
        )
    });
    registry.mount(|_| LocationObserver);
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: vec![DiffRow::Add {
                new_line: 1,
                text: "+éx".to_owned(),
            }],
            old_content: None,
            new_content: None,
        })
        .unwrap();

    let first_character = dispatch_key(&mut registry, diff_target, Key::Char('l'));
    let second_character = dispatch_key(&mut registry, diff_target, Key::Char('l'));
    let end_of_line = dispatch_key(&mut registry, diff_target, Key::Char('l'));
    let previous_character = dispatch_key(&mut registry, diff_target, Key::Char('h'));

    assert_eq!(output_texts(first_character), ["column:2"]);
    assert_eq!(output_texts(second_character), ["column:3"]);
    assert_eq!(output_texts(end_of_line), ["column:3"]);
    assert_eq!(output_texts(previous_character), ["column:2"]);
}

#[test]
fn source_shortcuts_move_on_deleted_lines() {
    let mut registry = ComponentEventBus::new();
    let reviewable_files = ReviewableFiles::default();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    let diff_target = registry.mount(|events| {
        DiffComponent::new(
            events,
            reviewable_files,
            SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White),
            PathBuf::new(),
            Theme::default().palette,
        )
    });
    registry.mount(|_| LocationObserver);
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: vec![DiffRow::Delete {
                old_line: 1,
                text: "-old".to_owned(),
            }],
            old_content: None,
            new_content: None,
        })
        .unwrap();

    let right = dispatch_key(&mut registry, diff_target, Key::Char('l'));

    assert_eq!(output_texts(right), ["column:1"]);
}

#[test]
fn word_under_cursor_search_does_nothing_on_punctuation() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    registry.mount(|_| DecorationObserver);
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: vec![DiffRow::Add {
                new_line: 1,
                text: "+word, next".to_owned(),
            }],
            old_content: None,
            new_content: None,
        })
        .unwrap();
    registry
        .dispatch_hovered_input(
            &EventEnvelope::new(pointer_input(
                PointerInputKind::Click { insert: false },
                1,
                10,
            )),
            diff_target,
        )
        .unwrap();

    let results = dispatch_key(&mut registry, diff_target, Key::Char('*'));

    assert_eq!(decoration_texts(results), ["notice:;search:"]);
}

#[test]
fn word_under_cursor_search_marks_files_with_the_selected_word() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    registry.mount(|_| DecorationObserver);
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: vec![DiffRow::Add {
                new_line: 1,
                text: "+word, next".to_owned(),
            }],
            old_content: None,
            new_content: None,
        })
        .unwrap();
    registry
        .dispatch_hovered_input(
            &EventEnvelope::new(pointer_input(
                PointerInputKind::Click { insert: false },
                1,
                4,
            )),
            diff_target,
        )
        .unwrap();

    let results = dispatch_key(&mut registry, diff_target, Key::Char('*'));

    assert_eq!(decoration_texts(results), ["notice:;search:src/lib.rs"]);
}

#[test]
fn cancelled_search_restores_its_origin_without_recording_a_jump() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    registry.mount(|_| FullLocationObserver);
    publish_repository(&mut registry, "checkpoint");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows: (1..=20)
                .map(|line| DiffRow::Context {
                    old_line: line,
                    new_line: line,
                    text: format!(" line {line}"),
                })
                .collect(),
            old_content: None,
            new_content: None,
        })
        .unwrap();

    dispatch_key(&mut registry, diff_target, Key::Char('/'));
    for character in "line 10".chars() {
        dispatch_key(&mut registry, diff_target, Key::Char(character));
    }
    let cancelled = dispatch_key(&mut registry, diff_target, Key::Escape);
    let previous = dispatch_key(&mut registry, diff_target, Key::PreviousLocation);

    assert!(output_texts(cancelled).contains(&"location:src/lib.rs:0:0".to_owned()));
    assert!(output_texts(previous).is_empty());
}

#[test]
fn repository_refresh_preserves_mouse_scrolled_viewport() {
    let (mut registry, reviewable_files, diff_target) = registry_with_observer();
    reviewable_files.replace(["src/lib.rs".to_owned()].into());
    publish_repository(&mut registry, "first");
    registry
        .publish(FileSelected {
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(DiffViewportChanged {
            width: 80,
            height: 8,
        })
        .unwrap();
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "first"),
            path: "src/lib.rs".to_owned(),
            rows: context_rows(60),
            old_content: None,
            new_content: None,
        })
        .unwrap();
    registry
        .dispatch_hovered_input(
            &EventEnvelope::new(PointerInput {
                kind: PointerInputKind::Scroll(20),
                position: None,
            }),
            diff_target,
        )
        .unwrap();
    let first_source_line = first_rendered_source_line(&registry, diff_target);
    assert!(
        first_source_line.contains("line 21"),
        "unexpected first source line: {first_source_line:?}"
    );

    let refresh_results = registry
        .publish(repository_event("second"))
        .expect("repository refresh must dispatch");
    let refresh_actions = refresh_results
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .filter(|action| matches!(action, Action::LoadDiff { .. }))
        .collect::<Vec<_>>();
    assert_eq!(
        refresh_actions,
        vec![Action::LoadDiff {
            review_checkpoint: ReviewCheckpoint::new("change", "second"),
            path: "src/lib.rs".to_owned(),
        }]
    );
    registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "second"),
            path: "src/lib.rs".to_owned(),
            rows: rows_with_insertion_before_context(60),
            old_content: None,
            new_content: None,
        })
        .unwrap();

    let first_source_line = first_rendered_source_line(&registry, diff_target);
    assert!(
        first_source_line.contains("line 21"),
        "unexpected first source line: {first_source_line:?}"
    );
}

#[test]
fn location_shortcuts_restore_semantic_file_jumps() {
    let mut registry = ComponentEventBus::new();
    let reviewable_files = ReviewableFiles::default();
    reviewable_files.replace(["first.rs".to_owned(), "second.rs".to_owned()].into());
    let diff_target = registry.mount(|events| {
        DiffComponent::new(
            events,
            reviewable_files,
            SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White),
            PathBuf::new(),
            Theme::default().palette,
        )
    });
    registry.mount(|_| ReviewPathObserver);
    registry
        .publish(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            files: ["first.rs", "second.rs"]
                .into_iter()
                .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
                .collect(),
        })
        .unwrap();
    registry
        .publish(FileSelected {
            path: "first.rs".to_owned(),
        })
        .unwrap();
    registry
        .publish(FileSelected {
            path: "second.rs".to_owned(),
        })
        .unwrap();

    let previous = dispatch_key(&mut registry, diff_target, Key::PreviousLocation);
    let next = dispatch_key(&mut registry, diff_target, Key::NextLocation);

    assert!(output_texts(previous).contains(&"path:first.rs".to_owned()));
    assert!(output_texts(next).contains(&"path:second.rs".to_owned()));
}

#[test]
fn location_history_skips_reviewed_files() {
    let (mut registry, reviewable_files, diff_target) = history_registry();
    select_files(&mut registry, ["a.rs", "b.rs", "c.rs"]);
    reviewable_files.replace(["a.rs".to_owned(), "c.rs".to_owned()].into());

    let previous = dispatch_key(&mut registry, diff_target, Key::PreviousLocation);
    let next = dispatch_key(&mut registry, diff_target, Key::NextLocation);

    assert!(output_texts(previous).contains(&"path:a.rs".to_owned()));
    assert!(output_texts(next).contains(&"path:c.rs".to_owned()));
}

#[test]
fn a_new_jump_after_back_keeps_the_later_history() {
    let (mut registry, _, diff_target) = history_registry();
    select_files(&mut registry, ["a.rs", "b.rs", "c.rs"]);
    dispatch_key(&mut registry, diff_target, Key::PreviousLocation);
    dispatch_key(&mut registry, diff_target, Key::PreviousLocation);
    registry
        .publish(FileSelected {
            path: "a.rs".to_owned(),
        })
        .unwrap();

    let next = dispatch_key(&mut registry, diff_target, Key::NextLocation);

    assert!(output_texts(next).contains(&"path:b.rs".to_owned()));
}

fn registry_with_observer() -> (ComponentEventBus<Action>, ReviewableFiles, ComponentTarget) {
    let mut registry = ComponentEventBus::new();
    let reviewable_files = ReviewableFiles::default();
    let diff_target = registry.mount(|context| {
        DiffComponent::new(
            context,
            reviewable_files.clone(),
            SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White),
            PathBuf::new(),
            Theme::default().palette,
        )
    });
    registry.mount(|_| ViewportObserver);
    (registry, reviewable_files, diff_target)
}

fn pointer_input(kind: PointerInputKind, row: u16, column: u16) -> PointerInput {
    PointerInput {
        kind,
        position: Some(PointerPosition {
            terminal_column: column,
            terminal_row: row,
            component_column: column,
            component_row: row,
        }),
    }
}

fn rendered_diff(registry: &ComponentEventBus<Action>, target: ComponentTarget) -> String {
    rendered_diff_lines(registry, target).concat()
}

fn rendered_diff_lines(
    registry: &ComponentEventBus<Action>,
    target: ComponentTarget,
) -> Vec<String> {
    let area = Rect::new(0, 0, 80, 12);
    let mut buffer = Buffer::empty(area);
    registry.get::<DiffComponent>(target).unwrap().render(
        area,
        &mut buffer,
        Theme::default().palette,
        true,
        None,
    );
    buffer
        .content()
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(ratatui::buffer::Cell::symbol).collect())
        .collect()
}

fn dispatch_key(
    registry: &mut ComponentEventBus<Action>,
    diff_target: ComponentTarget,
    key: Key,
) -> Vec<DispatchResult<Action>> {
    registry
        .dispatch_input(&EventEnvelope::new(key), diff_target)
        .expect("key input must dispatch")
        .into_results()
}

fn dispatch_global_shortcut(
    registry: &mut ComponentEventBus<Action>,
    focused_target: ComponentTarget,
    prefix: char,
    key: char,
) {
    let prefix_dispatch = registry
        .dispatch_input(&EventEnvelope::new(Key::Char(prefix)), focused_target)
        .expect("shortcut prefix must dispatch");
    assert!(prefix_dispatch.global_input_pending());
    registry
        .dispatch_global_input(&EventEnvelope::new(Key::Char(key)))
        .expect("global shortcut must dispatch");
}

fn publish_repository(registry: &mut ComponentEventBus<Action>, checkpoint: &str) {
    registry
        .publish_envelope(EventEnvelope::new(repository_event(checkpoint)))
        .expect("repository must dispatch");
}

fn repository_event(checkpoint: &str) -> RepositoryFilesChanged {
    RepositoryFilesChanged {
        review_checkpoint: ReviewCheckpoint::new("change", checkpoint),
        files: vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    }
}

fn changed_rows() -> Vec<DiffRow> {
    vec![
        DiffRow::Hunk {
            old_start: 1,
            old_count: 0,
            new_start: 1,
            new_count: 1,
        },
        DiffRow::Add {
            new_line: 1,
            text: "+changed".to_owned(),
        },
    ]
}

fn two_hunk_rows() -> Vec<DiffRow> {
    vec![
        DiffRow::Hunk {
            old_start: 1,
            old_count: 6,
            new_start: 1,
            new_count: 7,
        },
        DiffRow::Add {
            new_line: 1,
            text: "+first change".to_owned(),
        },
        DiffRow::Context {
            old_line: 1,
            new_line: 2,
            text: " line 1".to_owned(),
        },
        DiffRow::Context {
            old_line: 2,
            new_line: 3,
            text: " line 2".to_owned(),
        },
        DiffRow::Context {
            old_line: 3,
            new_line: 4,
            text: " line 3".to_owned(),
        },
        DiffRow::Context {
            old_line: 4,
            new_line: 5,
            text: " line 4".to_owned(),
        },
        DiffRow::Context {
            old_line: 5,
            new_line: 6,
            text: " line 5".to_owned(),
        },
        DiffRow::Context {
            old_line: 6,
            new_line: 7,
            text: " line 6".to_owned(),
        },
        DiffRow::Hunk {
            old_start: 20,
            old_count: 1,
            new_start: 21,
            new_count: 2,
        },
        DiffRow::Add {
            new_line: 21,
            text: "+second change".to_owned(),
        },
        DiffRow::Context {
            old_line: 20,
            new_line: 22,
            text: " line 20".to_owned(),
        },
    ]
}

fn context_rows(count: u32) -> Vec<DiffRow> {
    (1..=count)
        .map(|line| DiffRow::Context {
            old_line: line,
            new_line: line,
            text: format!(" line {line}"),
        })
        .collect()
}

fn rows_with_insertion_before_context(count: u32) -> Vec<DiffRow> {
    std::iter::once(DiffRow::Add {
        new_line: 1,
        text: "+inserted".to_owned(),
    })
    .chain((1..=count).map(|line| DiffRow::Context {
        old_line: line,
        new_line: line.saturating_add(1),
        text: format!(" line {line}"),
    }))
    .collect()
}

fn first_rendered_source_line(
    registry: &ComponentEventBus<Action>,
    diff_target: ComponentTarget,
) -> String {
    rendered_diff_lines(registry, diff_target)
        .into_iter()
        .find(|line| line.contains("line "))
        .expect("the rendered diff must contain a source line")
}

fn history_registry() -> (ComponentEventBus<Action>, ReviewableFiles, ComponentTarget) {
    let mut registry = ComponentEventBus::new();
    let reviewable_files = ReviewableFiles::default();
    reviewable_files.replace(
        ["a.rs", "b.rs", "c.rs"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    );
    let diff_target = registry.mount(|events| {
        DiffComponent::new(
            events,
            reviewable_files.clone(),
            SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White),
            PathBuf::new(),
            Theme::default().palette,
        )
    });
    registry.mount(|_| ReviewPathObserver);
    registry
        .publish(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            files: ["a.rs", "b.rs", "c.rs"]
                .into_iter()
                .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
                .collect(),
        })
        .unwrap();
    (registry, reviewable_files, diff_target)
}

fn select_files<const COUNT: usize>(
    registry: &mut ComponentEventBus<Action>,
    paths: [&str; COUNT],
) {
    for path in paths {
        registry
            .publish(FileSelected {
                path: path.to_owned(),
            })
            .unwrap();
    }
}

struct ViewportObserver;

impl ViewportObserver {
    #[allow(clippy::unused_self)]
    fn viewports_changed(&mut self, event: &DisplayedDiffViewportsChanged) -> Vec<Action> {
        let path = event
            .viewports
            .first()
            .map_or("none", |viewport| viewport.path.as_str());
        let changed_rows = event
            .viewports
            .iter()
            .flat_map(|viewport| &viewport.rows)
            .filter(|row| row.changed)
            .count();
        vec![Action::Output {
            text: format!("{path}:{}:{changed_rows}", event.current_row),
        }]
    }
}

impl Component<Action> for ViewportObserver {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::viewports_changed);
    }
}

struct DecorationObserver;

impl DecorationObserver {
    #[allow(clippy::unused_self)]
    fn decorations_changed(&mut self, event: &FileDecorationsChanged) -> Vec<Action> {
        vec![Action::Output {
            text: format!(
                "notice:{};search:{}",
                event.notice_paths.join(","),
                event.search_match_paths.join(",")
            ),
        }]
    }
}

impl Component<Action> for DecorationObserver {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::decorations_changed);
    }
}

struct LocationObserver;

impl LocationObserver {
    #[allow(clippy::unused_self)]
    fn location_changed(&mut self, event: &CurrentReviewLocationChanged) -> Vec<Action> {
        let Some(ReviewLocation::LoadedDocument { column, .. }) = &event.location else {
            return Vec::new();
        };
        vec![Action::Output {
            text: format!("column:{column}"),
        }]
    }
}

struct ReviewPathObserver;

struct FullLocationObserver;

struct SearchStatusObserver;

struct ToastObserver;

impl ToastObserver {
    #[allow(clippy::unused_self)]
    fn toast_requested(&mut self, event: &ToastRequested) -> Vec<Action> {
        vec![Action::Output {
            text: format!("toast:{}", event.text),
        }]
    }
}

impl Component<Action> for ToastObserver {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::toast_requested);
    }
}

impl FullLocationObserver {
    #[allow(clippy::unused_self)]
    fn location_changed(&mut self, event: &CurrentReviewLocationChanged) -> Vec<Action> {
        let Some(ReviewLocation::LoadedDocument {
            path,
            cursor,
            column,
            ..
        }) = &event.location
        else {
            return Vec::new();
        };
        vec![Action::Output {
            text: format!("location:{path}:{cursor}:{column}"),
        }]
    }
}

impl Component<Action> for FullLocationObserver {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::location_changed);
    }
}

impl SearchStatusObserver {
    #[allow(clippy::unused_self)]
    fn search_changed(&mut self, event: &SearchStatusChanged) -> Vec<Action> {
        vec![Action::Output {
            text: format!(
                "search:{}:{}/{}",
                event.query.as_deref().unwrap_or_default(),
                event.current_match,
                event.total_matches
            ),
        }]
    }
}

impl Component<Action> for SearchStatusObserver {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::search_changed);
    }
}

impl ReviewPathObserver {
    #[allow(clippy::unused_self)]
    fn location_changed(&mut self, event: &CurrentReviewLocationChanged) -> Vec<Action> {
        let Some(ReviewLocation::LoadedDocument { path, .. }) = &event.location else {
            return Vec::new();
        };
        vec![Action::Output {
            text: format!("path:{path}"),
        }]
    }
}

impl Component<Action> for ReviewPathObserver {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::location_changed);
    }
}

impl Component<Action> for LocationObserver {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::location_changed);
    }
}

fn output_texts(results: Vec<DispatchResult<Action>>) -> Vec<String> {
    results
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .filter_map(|action| match action {
            Action::Output { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

fn decoration_texts(results: Vec<DispatchResult<Action>>) -> Vec<String> {
    output_texts(results)
        .into_iter()
        .filter(|text| text.starts_with("notice:"))
        .collect()
}
