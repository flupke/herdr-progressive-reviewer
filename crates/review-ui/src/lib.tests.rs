use std::fmt::Write;
use std::path::{Path, PathBuf};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use review_guide::{
    GuideItem, GuideItemStatus, GuideLineRange, GuideScope, GuideTarget, ReviewCheckpoint,
};
use review_lsp::{Event, Operation, SourceLocation};
use review_repository::diff::DiffRow;
use toasts::ToastId;

use crate::app::{
    Action, ActivePopup, ContextMenu, DiffControl, DragState, Focus, Key, Message, PaneLayout,
    PendingReview, ReviewApp, ReviewFile, Search, Selection, SourceLoadMode,
};
use review_state::{ReviewState, ReviewStatus};

#[test]
fn pane_layout_defines_all_visible_and_interactive_boundaries() {
    let wide = PaneLayout::new(100, 20, Some(30));
    assert!(wide.is_wide());
    assert_eq!(wide.file_width, 30);
    assert_eq!(wide.body_height(), 18);
    assert_eq!(wide.page_rows(), 16);
    assert_eq!(wide.diff_content_width(), 68);
    assert_eq!(wide.diff_content_start_column(), 31);

    assert!(wide.contains_body(0, 1));
    assert!(wide.contains_body(99, 17));
    assert!(!wide.contains_body(100, 1));
    assert!(!wide.contains_body(0, 0));
    assert!(wide.contains_body(0, 18));
    assert!(!wide.contains_body(0, 19));
    assert!(!wide.contains_pane_content(1));
    assert!(wide.contains_pane_content(2));
    assert!(wide.contains_pane_content(16));
    assert!(wide.contains_pane_content(17));
    assert!(!wide.contains_pane_content(18));

    assert_eq!(wide.focus_at(Focus::Diff, 29, 1), Some(Focus::Files));
    assert_eq!(wide.focus_at(Focus::Files, 30, 1), Some(Focus::Diff));
    assert_eq!(wide.focus_at(Focus::Files, 30, 0), None);
    assert!(!wide.is_separator(28, 1));
    assert!(wide.is_separator(29, 1));
    assert!(wide.is_separator(30, 1));
    assert!(wide.is_separator(31, 1));
    assert!(!wide.is_separator(30, 0));

    let narrow = PaneLayout::new(60, 10, Some(30));
    assert!(!narrow.is_wide());
    assert_eq!(narrow.file_width, 60);
    assert_eq!(narrow.diff_content_width(), 58);
    assert_eq!(narrow.diff_content_start_column(), 1);
    assert_eq!(narrow.focus_at(Focus::Files, 59, 1), Some(Focus::Files));
    assert_eq!(narrow.focus_at(Focus::Diff, 59, 1), Some(Focus::Diff));
}

#[test]
fn diff_controls_have_exact_click_targets() {
    assert!(!DiffControl::visible(31, None));
    assert!(DiffControl::visible(32, None));

    let width = 80;
    let controls = (0..width)
        .filter_map(|column| DiffControl::at(width, column, None).map(|control| (column, control)))
        .collect::<Vec<_>>();
    assert_eq!(
        controls,
        vec![
            (70, DiffControl::ExpandAll),
            (71, DiffControl::ExpandAll),
            (72, DiffControl::ExpandAll),
            (73, DiffControl::ExpandAll),
            (75, DiffControl::ContractAll),
            (76, DiffControl::ContractAll),
            (77, DiffControl::ContractAll),
            (78, DiffControl::ContractAll),
        ]
    );

    let mut temporary = ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed);
    temporary.temporary = true;
    assert!(!DiffControl::visible(width, Some(&temporary)));
}

#[test]
fn review_guide_shortcuts_select_file_and_all_scopes() {
    let mut app = ReviewApp {
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
        ..ReviewApp::default()
    };

    assert_eq!(app.update(Message::Key(Key::Char('r'))), Action::None);
    assert_eq!(
        app.update(Message::Key(Key::Char('f'))),
        Action::GenerateReviewGuide {
            scope: GuideScope::File {
                path: "src/lib.rs".to_owned()
            }
        }
    );
    assert_eq!(app.update(Message::Key(Key::Char('r'))), Action::None);
    assert_eq!(
        app.update(Message::Key(Key::Char('a'))),
        Action::GenerateReviewGuide {
            scope: GuideScope::All
        }
    );

    app.guide_spinner_frame = Some(0);
    assert_eq!(app.update(Message::Key(Key::Char('r'))), Action::None);
    assert!(app.pending_shortcut_prefix.is_some());
    assert_eq!(app.update(Message::Key(Key::Char('a'))), Action::None);
    assert_eq!(app.pending_shortcut_prefix, None);
}

#[test]
fn escape_closes_shortcut_help_and_clears_transient_state() {
    let mut app = ReviewApp {
        active_popup: Some(ActivePopup::ShortcutHelp),
        selection: Some(Selection {
            anchor: 1,
            cursor: 2,
            fixed: false,
        }),
        search: Some(Search {
            query: "term".to_owned(),
            origin: 0,
            origin_location: None,
            editing: false,
            pending: Vec::new(),
        }),
        ..ReviewApp::default()
    };

    assert_eq!(app.update(Message::Key(Key::Escape)), Action::None);
    assert!(app.active_popup.is_none());
    assert!(app.selection.is_none());
    assert!(app.search.is_none());
}

#[test]
fn review_guide_comment_shortcuts_wrap_between_files() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![
            ReviewFile::new("src/first.rs", ReviewStatus::Unreviewed),
            ReviewFile::new("src/second.rs", ReviewStatus::Unreviewed),
        ],
    });
    for path in ["src/first.rs", "src/second.rs"] {
        app.update(Message::DiffLoaded {
            commit_id: "commit".to_owned(),
            path: path.to_owned(),
            rows: vec![
                DiffRow::Hunk {
                    old_start: 0,
                    old_count: 0,
                    new_start: 1,
                    new_count: 1,
                },
                DiffRow::Add {
                    new_line: 1,
                    text: "+changed".to_owned(),
                },
            ]
            .into_iter()
            .chain(
                (path == "src/first.rs")
                    .then_some([
                        DiffRow::Hunk {
                            old_start: 1,
                            old_count: 0,
                            new_start: 10,
                            new_count: 1,
                        },
                        DiffRow::Add {
                            new_line: 10,
                            text: "+later".to_owned(),
                        },
                    ])
                    .into_iter()
                    .flatten(),
            )
            .collect(),
            old_content: None,
            new_content: None,
        });
    }
    app.update(Message::ReviewGuideLoaded {
        review_checkpoint: ReviewCheckpoint::new("change", "commit"),
        items: ["src/first.rs", "src/second.rs"]
            .into_iter()
            .map(|path| GuideItem {
                target: GuideTarget::Hunks {
                    path: path.to_owned(),
                    first_hunk: 1,
                    last_hunk: 1,
                },
                text: "Comment".to_owned(),
                status: GuideItemStatus::Matched,
            })
            .chain([
                GuideItem {
                    target: GuideTarget::Hunks {
                        path: "src/first.rs".to_owned(),
                        first_hunk: 2,
                        last_hunk: 2,
                    },
                    text: "Later comment".to_owned(),
                    status: GuideItemStatus::Matched,
                },
                GuideItem {
                    target: GuideTarget::Hunks {
                        path: "src/first.rs".to_owned(),
                        first_hunk: 3,
                        last_hunk: 3,
                    },
                    text: "Unmapped comment".to_owned(),
                    status: GuideItemStatus::Stale,
                },
            ])
            .collect(),
    });
    app.focus = Focus::Diff;
    app.files[0].cursor = 0;

    assert_eq!(app.update(Message::Key(Key::Char(']'))), Action::None);
    assert_eq!(app.update(Message::Key(Key::Char('r'))), Action::None);
    assert_eq!((app.selected_file, app.files[0].cursor), (0, 1));

    assert_eq!(app.update(Message::Key(Key::Char(']'))), Action::None);
    assert_eq!(app.update(Message::Key(Key::Char('r'))), Action::None);
    assert_eq!((app.selected_file, app.files[1].cursor), (1, 0));

    assert_eq!(app.update(Message::Key(Key::Char('['))), Action::None);
    assert_eq!(app.update(Message::Key(Key::Char('r'))), Action::None);
    assert_eq!((app.selected_file, app.files[0].cursor), (0, 1));
}

#[test]
fn guide_jump_waits_for_the_target_file_and_finishes_after_it_loads() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![
            ReviewFile::new("src/first.rs", ReviewStatus::Unreviewed),
            ReviewFile::new("src/second.rs", ReviewStatus::Unreviewed),
        ],
    });
    app.update(Message::ReviewGuideLoaded {
        review_checkpoint: ReviewCheckpoint::new("change", "commit"),
        items: vec![GuideItem {
            target: GuideTarget::Lines {
                path: "src/second.rs".to_owned(),
                old: None,
                new: Some(GuideLineRange {
                    first_line: 10,
                    last_line: 10,
                }),
            },
            text: "Comment".to_owned(),
            status: GuideItemStatus::Matched,
        }],
    });

    app.update(Message::Key(Key::Char(']')));
    assert_eq!(
        app.update(Message::Key(Key::Char('r'))),
        Action::LoadDiff {
            commit_id: "commit".to_owned(),
            path: "src/second.rs".to_owned(),
        }
    );
    assert!(app.pending_guide_jump.is_some());
    app.finish_pending_guide_jump("src/wrong.rs");
    assert!(app.pending_guide_jump.is_some());

    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/second.rs".to_owned(),
        rows: vec![
            DiffRow::Hunk {
                old_start: 1,
                old_count: 0,
                new_start: 10,
                new_count: 1,
            },
            DiffRow::Add {
                new_line: 10,
                text: "+target".to_owned(),
            },
        ],
        old_content: None,
        new_content: None,
    });
    assert!(app.pending_guide_jump.is_none());
    assert_eq!(app.selected_file, 1);
    assert_eq!(app.selected().unwrap().cursor, 0);
    assert_eq!(app.selected().unwrap().scroll, 0);
}

#[test]
fn unavailable_unloaded_guide_target_does_not_leave_a_pending_jump() {
    let mut app = ReviewApp {
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
        guide_items: vec![GuideItem {
            target: GuideTarget::File {
                path: "src/lib.rs".to_owned(),
            },
            text: "Comment".to_owned(),
            status: GuideItemStatus::Matched,
        }],
        ..ReviewApp::default()
    };
    app.files[0].loading = true;

    app.update(Message::Key(Key::Char(']')));
    assert_eq!(app.update(Message::Key(Key::Char('r'))), Action::None);
    assert!(app.pending_guide_jump.is_none());
}

#[test]
fn interactive_search_moves_and_repeats_from_the_diff_cursor() {
    let mut app = search_test_app();

    assert_eq!(
        app.update(Message::Key(Key::Char('/'))),
        super::Action::LoadDiffs {
            commit_id: "commit".to_owned(),
            paths: vec!["src/unopened.rs".to_owned()],
        }
    );
    app.update(Message::Key(Key::Char('n')));
    assert_eq!(app.selected().unwrap().cursor, 1);
    app.update(Message::Key(Key::Char('e')));
    assert_eq!(app.selected().unwrap().cursor, 3);
    for character in "edle".chars() {
        app.update(Message::Key(Key::Char(character)));
    }
    assert!(app.file_matches_search(0));
    assert!(app.file_matches_search(1));
    assert!(!app.file_matches_search(2));
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    assert_search_highlighted(&mut terminal, &app);
    app.update(Message::Key(Key::Enter));
    app.update(Message::Key(Key::Char('n')));
    app.update(Message::Key(Key::Char('n')));
    assert_eq!(app.selected().unwrap().cursor, 3);
    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/unopened.rs".to_owned(),
        rows: vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: " needle in the unopened file".to_owned(),
        }],
        old_content: None,
        new_content: None,
    });
    assert_eq!((app.selected_file, app.selected().unwrap().cursor), (1, 1));
    assert_search_highlighted(&mut terminal, &app);
    app.update(Message::Key(Key::Char('p')));
    assert_eq!((app.selected_file, app.selected().unwrap().cursor), (0, 5));
    app.update(Message::Key(Key::Escape));
    app.update(Message::Key(Key::Char('n')));
    assert_eq!(app.selected().unwrap().cursor, 5);

    app.files[0].cursor = 0;
    app.update(Message::Key(Key::Char('/')));
    app.update(Message::Key(Key::Char('T')));
    assert_eq!(app.selected().unwrap().cursor, 0);
    app.update(Message::Key(Key::Escape));
    app.update(Message::Key(Key::Char('/')));
    app.update(Message::Key(Key::Char('t')));
    assert_eq!(app.selected().unwrap().cursor, 5);
}

#[test]
fn repeated_search_centers_each_match() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: (0..40)
            .map(|line| DiffRow::Context {
                old_line: line + 1,
                new_line: line + 1,
                text: if line == 10 || line == 30 {
                    " match".to_owned()
                } else {
                    format!(" line_{line}")
                },
            })
            .collect(),
        old_content: None,
        new_content: None,
    });
    for key in [
        Key::Char('/'),
        Key::Char('m'),
        Key::Char('a'),
        Key::Char('t'),
        Key::Char('c'),
        Key::Char('x'),
        Key::Backspace,
        Key::Char('h'),
        Key::Enter,
        Key::Char('n'),
    ] {
        app.update(Message::Key(key));
    }
    let page = app.page_rows();
    let file = app.selected().unwrap();
    assert_eq!(file.cursor, 30);
    assert_eq!(file.scroll, 30_usize.saturating_sub(page / 2));

    app.update(Message::Key(Key::Char('p')));
    let file = app.selected().unwrap();
    assert_eq!(file.cursor, 10);
    assert_eq!(file.scroll, 10_usize.saturating_sub(page / 2));

    app.update(Message::Key(Key::First));
    assert_eq!(app.selected().unwrap().cursor, 0);
}

fn app_with_visible_current_source() -> ReviewApp {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: " fn target() {}".to_owned(),
        }],
        old_content: Some(b"fn target() {}\n".to_vec()),
        new_content: Some(b"fn target() {}\n".to_vec()),
    });
    app.focus = Focus::Diff;
    app
}

#[test]
fn lsp_keys_use_the_visible_current_source() {
    let mut app = app_with_visible_current_source();
    assert!(matches!(
        app.update(Message::Key(Key::Char('K'))),
        Action::Lsp {
            operation: Operation::Hover,
            query: review_lsp::Query {
                path,
                line: 0,
                byte_column: 0,
                expected_line,
                snapshot_id,
                ..
            },
        }
        if path == Path::new("src/lib.rs")
            && expected_line == "fn target() {}"
            && snapshot_id == "commit"
    ));
    assert_eq!(app.update(Message::Key(Key::Char('g'))), Action::None);
    assert!(matches!(
        app.update(Message::Key(Key::Char('d'))),
        Action::Lsp {
            operation: Operation::Definition,
            ..
        }
    ));
    assert_eq!(app.update(Message::Key(Key::Char('g'))), Action::None);
    assert_eq!(app.update(Message::Key(Key::Char('R'))), Action::RestartLsp);
    assert_eq!(app.update(Message::Key(Key::Char('g'))), Action::None);
    assert!(matches!(
        app.update(Message::Key(Key::Char('r'))),
        Action::Lsp {
            operation: Operation::References,
            ..
        }
    ));
}

#[test]
fn source_word_motion_only_changes_the_diff_column() {
    let mut app = app_with_visible_current_source();
    app.files[0].column = 0;
    app.move_word(true);
    assert_eq!(app.files[0].column, 3);
    app.files[0].column = 5;
    app.move_word(false);
    assert_eq!(app.files[0].column, 3);

    app.focus = Focus::Files;
    app.files[0].column = 5;
    for key in [
        Key::Char('h'),
        Key::Char('l'),
        Key::Char('w'),
        Key::Char('b'),
        Key::Char('0'),
        Key::Char('$'),
    ] {
        assert_eq!(app.update(Message::Key(key)), Action::None);
        assert_eq!(app.files[0].column, 5);
    }

    app.focus = Focus::Diff;
    assert_eq!(app.update(Message::Key(Key::Char('h'))), Action::None);
    assert_eq!(app.files[0].column, 4);
    assert_eq!(app.update(Message::Key(Key::Char('l'))), Action::None);
    assert_eq!(app.files[0].column, 5);
    assert_eq!(app.update(Message::Key(Key::Char('0'))), Action::None);
    assert_eq!(app.files[0].column, 0);
    assert_eq!(app.update(Message::Key(Key::Char('$'))), Action::None);
    assert_eq!(app.files[0].column, "fn target() {}".len());
    app.files[0].column = 0;
    assert_eq!(app.update(Message::Key(Key::Char('w'))), Action::None);
    assert_eq!(app.files[0].column, 3);
    app.files[0].column = 5;
    assert_eq!(app.update(Message::Key(Key::Char('b'))), Action::None);
    assert_eq!(app.files[0].column, 3);
}

#[test]
fn source_mouse_actions_use_the_visible_current_source() {
    let mut app = app_with_visible_current_source();
    assert_eq!(
        app.update(Message::MouseControlClick { column: 30, row: 2 }),
        Action::None
    );
    assert!(app.context_menu.is_some());
    app.context_menu = None;
    assert!(matches!(
        app.update(Message::MouseControlClick { column: 3, row: 3 }),
        Action::Output { text, .. } if text == "src/lib.rs"
    ));
    assert_eq!(app.mouse_double_click(30, 3), Action::None);
    assert!(matches!(
        app.mouse_double_click(3, 3),
        Action::SetReviewed {
            path,
            reviewed: true,
        } if path == "src/lib.rs"
    ));
}

#[test]
fn hover_keys_scroll_in_both_directions_and_close_the_hover() {
    let mut app = ReviewApp {
        hover: Some("documentation".to_owned()),
        hover_scroll: 2,
        ..ReviewApp::default()
    };

    assert_eq!(app.update(Message::Key(Key::Up)), Action::None);
    assert_eq!(app.hover_scroll, 1);
    assert_eq!(app.update(Message::Key(Key::Down)), Action::None);
    assert_eq!(app.hover_scroll, 2);
    assert_eq!(app.update(Message::Key(Key::Char('j'))), Action::None);
    assert_eq!(app.hover_scroll, 3);
    assert_eq!(app.update(Message::Key(Key::Char('k'))), Action::None);
    assert_eq!(app.hover_scroll, 2);
    assert_eq!(app.update(Message::Key(Key::Escape)), Action::None);
    assert!(app.hover.is_none());
}

#[test]
fn visual_mode_only_starts_in_the_diff_pane() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: (1..=40)
            .map(|line| DiffRow::Context {
                old_line: line,
                new_line: line,
                text: format!(" line {line}"),
            })
            .collect(),
        old_content: Some(b"fn target() {}\n".to_vec()),
        new_content: Some(b"fn target() {}\n".to_vec()),
    });
    app.focus = Focus::Files;

    assert_eq!(app.update(Message::Key(Key::Char('v'))), Action::None);
    assert!(app.selection.is_none());
    assert_eq!(app.update(Message::Key(Key::Char('K'))), Action::None);

    app.focus = Focus::Diff;
    app.update(Message::Key(Key::Char('v')));
    assert_eq!(app.selection.unwrap().range(), 0..=0);
    assert!(!app.selection.unwrap().fixed);
    app.update(Message::Key(Key::Down));
    assert_eq!(app.selection.unwrap().range(), 0..=1);
    app.update(Message::Key(Key::Char('v')));
    assert!(app.selection.unwrap().fixed);
    app.update(Message::Key(Key::Down));
    assert_eq!(app.selection.unwrap().range(), 0..=1);
    app.update(Message::Key(Key::Char('v')));
    assert_eq!(app.selection.unwrap().range(), 2..=2);
    assert!(!app.selection.unwrap().fixed);
    app.update(Message::Key(Key::HalfPageDown));
    assert!(app.selection.unwrap().cursor > 2);

    app.width = 100;
    app.height = 20;
    app.file_width = Some(30);
    app.drag = DragState::Resize { moved: false };
    app.mouse_drag(40, 3);
    assert_eq!(app.file_width, Some(40));
    assert_eq!(app.drag, DragState::Resize { moved: true });
    app.mouse_drag(5, 0);
    assert_eq!(app.file_width, Some(40));

    app.file_width = Some(30);
    app.files[0].cursor = 0;
    app.drag = DragState::Select {
        anchor: 0,
        moved: false,
    };
    app.mouse_drag(40, 4);
    assert_eq!(app.files[0].cursor, 2);
    assert_eq!(app.selection.unwrap().range(), 0..=2);
    assert_eq!(
        app.drag,
        DragState::Select {
            anchor: 0,
            moved: true,
        }
    );
    app.mouse_drag(5, 5);
    assert_eq!(app.files[0].cursor, 2);
}

#[test]
fn repository_refresh_updates_paths_and_clears_checkpoint_state() {
    let mut app = ReviewApp {
        repository_root: PathBuf::from("/repository"),
        ..ReviewApp::default()
    };
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "first".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app.guide_items.push(GuideItem {
        target: GuideTarget::File {
            path: "src/lib.rs".to_owned(),
        },
        text: "Guide".to_owned(),
        status: GuideItemStatus::Matched,
    });
    app.guide_spinner_frame = Some(2);
    app.hover = Some("hover".to_owned());

    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "second".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });

    assert_eq!(
        app.files[0].disk_path.as_deref(),
        Some(Path::new("/repository/src/lib.rs"))
    );
    assert!(app.guide_items.is_empty());
    assert!(app.guide_spinner_frame.is_none());
    assert!(app.hover.is_none());
}

#[test]
fn new_review_unit_resets_transient_review_state() {
    let mut app = ReviewApp {
        change_id: "old".to_owned(),
        commit_id: "old".to_owned(),
        active_popup: Some(ActivePopup::CommitMessage),
        file_scroll: 4,
        focus: Focus::Diff,
        hover: Some("hover".to_owned()),
        review_in_flight: Some(PendingReview {
            path: "old.rs".to_owned(),
            previous_status: ReviewStatus::Unreviewed,
            optimistic_status: ReviewStatus::Reviewed,
            next_path: None,
        }),
        ..ReviewApp::default()
    };

    app.update(Message::FilesLoaded {
        change_id: "new".to_owned(),
        commit_id: "new".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("new.rs", ReviewStatus::Unreviewed)],
    });

    assert!(app.active_popup.is_none());
    assert_eq!(app.file_scroll, 0);
    assert_eq!(app.focus, Focus::Files);
    assert!(app.hover.is_none());
    assert!(app.review_in_flight.is_none());
}

#[test]
fn refresh_applies_an_optimistic_status_only_to_its_pending_file() {
    let mut app = ReviewApp {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        files: vec![
            ReviewFile::new("pending.rs", ReviewStatus::Unreviewed),
            ReviewFile::new("other.rs", ReviewStatus::Unreviewed),
        ],
        review_in_flight: Some(PendingReview {
            path: "pending.rs".to_owned(),
            previous_status: ReviewStatus::Unreviewed,
            optimistic_status: ReviewStatus::Reviewed,
            next_path: Some("other.rs".to_owned()),
        }),
        ..ReviewApp::default()
    };

    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![
            ReviewFile::new("pending.rs", ReviewStatus::Unreviewed),
            ReviewFile::new("other.rs", ReviewStatus::Unreviewed),
        ],
    });

    assert_eq!(app.files[0].status, ReviewStatus::Reviewed);
    assert_eq!(app.files[1].status, ReviewStatus::Unreviewed);
}

#[test]
fn successful_review_updates_only_the_named_file_and_reloads_it_when_selected() {
    let mut app = ReviewApp {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        files: vec![
            ReviewFile::new("other.rs", ReviewStatus::Reviewed),
            ReviewFile::new("target.rs", ReviewStatus::Reviewed),
        ],
        selected_file: 0,
        ..ReviewApp::default()
    };

    assert_eq!(
        app.update(Message::ReviewFinished {
            change_id: "change".to_owned(),
            path: "target.rs".to_owned(),
            result: Ok(ReviewState {
                status: ReviewStatus::Unreviewed,
                warning: None,
            }),
        }),
        Action::None
    );
    assert_eq!(app.files[0].status, ReviewStatus::Reviewed);
    assert_eq!(app.files[1].status, ReviewStatus::Unreviewed);

    app.selected_file = 1;
    assert_eq!(
        app.update(Message::ReviewFinished {
            change_id: "change".to_owned(),
            path: "target.rs".to_owned(),
            result: Ok(ReviewState {
                status: ReviewStatus::ChangedSinceReview,
                warning: None,
            }),
        }),
        Action::LoadDiff {
            commit_id: "commit".to_owned(),
            path: "target.rs".to_owned(),
        }
    );
}

#[test]
fn completed_optimistic_review_reloads_the_selected_next_file() {
    let pending = |optimistic_status, next_path: Option<&str>| PendingReview {
        path: "reviewed.rs".to_owned(),
        previous_status: ReviewStatus::Unreviewed,
        optimistic_status,
        next_path: next_path.map(str::to_owned),
    };
    let app = |pending_review| ReviewApp {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        files: vec![
            ReviewFile::new("reviewed.rs", ReviewStatus::Reviewed),
            ReviewFile::new("next.rs", ReviewStatus::Unreviewed),
        ],
        selected_file: 1,
        review_in_flight: Some(pending_review),
        ..ReviewApp::default()
    };
    let finish = |app: &mut ReviewApp, status| {
        app.update(Message::ReviewFinished {
            change_id: "change".to_owned(),
            path: "reviewed.rs".to_owned(),
            result: Ok(ReviewState {
                status,
                warning: None,
            }),
        })
    };

    let mut successful = app(pending(ReviewStatus::Reviewed, Some("next.rs")));
    assert_eq!(
        finish(&mut successful, ReviewStatus::Reviewed),
        Action::LoadDiff {
            commit_id: "commit".to_owned(),
            path: "next.rs".to_owned(),
        }
    );

    for (optimistic_status, result_status, next_path) in [
        (
            ReviewStatus::Unreviewed,
            ReviewStatus::Reviewed,
            Some("next.rs"),
        ),
        (
            ReviewStatus::Reviewed,
            ReviewStatus::Unreviewed,
            Some("next.rs"),
        ),
        (
            ReviewStatus::Reviewed,
            ReviewStatus::Reviewed,
            Some("other.rs"),
        ),
        (ReviewStatus::Reviewed, ReviewStatus::Reviewed, None),
    ] {
        let mut app = app(pending(optimistic_status, next_path));
        assert_eq!(finish(&mut app, result_status), Action::None);
    }
}

#[test]
fn location_overlay_click_uses_the_scrolled_screen_row() {
    let mut app = ReviewApp {
        width: 80,
        height: 20,
        focus: Focus::Files,
        commit_id: "commit".to_owned(),
        ..ReviewApp::default()
    };
    let locations = (0..6)
        .map(|line| source_location("src/lib.rs", line))
        .collect();
    app.update(Message::Lsp(Event::Locations {
        toast_id: ToastId::generate(),
        operation: Operation::References,
        snapshot_id: "commit".to_owned(),
        locations,
    }));
    app.locations.as_mut().unwrap().scroll = 1;

    let action = app.update(Message::MouseClick {
        column: 2,
        row: 5,
        insert_path: false,
    });

    assert_eq!(app.locations.as_ref().unwrap().selected, 4);
    assert!(matches!(action, Action::LoadSource { .. }));
}

#[test]
fn one_reference_still_opens_the_result_list() {
    let mut app = ReviewApp {
        commit_id: "commit".to_owned(),
        ..ReviewApp::default()
    };
    let location = source_location("src/lib.rs", 1);

    assert_eq!(
        app.update(Message::Lsp(Event::Locations {
            toast_id: ToastId::generate(),
            operation: Operation::References,
            snapshot_id: "commit".to_owned(),
            locations: vec![location.clone()],
        })),
        Action::LoadSource {
            snapshot_id: "commit".to_owned(),
            location,
            mode: SourceLoadMode::Preview,
        }
    );
    assert!(app.locations.is_some());
}

#[test]
fn definition_target_is_centered() {
    let mut app = ReviewApp::new(
        crate::Theme::default(),
        None,
        review_store::OutputTarget::default(),
        PathBuf::from("/repo"),
    );
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: (1..=40)
            .map(|line| DiffRow::Context {
                old_line: line,
                new_line: line,
                text: format!(" line_{line}"),
            })
            .collect(),
        old_content: None,
        new_content: None,
    });
    let mut location = source_location("src/lib.rs", 29);
    location.path = PathBuf::from("/repo/src/lib.rs");

    assert_eq!(app.accept_location(location), Action::None);
    let file = app.selected().unwrap();
    assert_eq!(file.cursor - file.scroll, app.page_rows() / 2);
}

#[test]
fn location_history_moves_in_both_directions_and_keeps_later_jumps() {
    let mut app = location_history_app(["a", "b", "c"], |name| {
        vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: format!(" fn {name}() {{}}"),
        }]
    });

    let location = |name| source_location(&format!("/repo/src/{name}.rs"), 0);
    assert_eq!(app.accept_location(location("b")), Action::None);
    assert_eq!(app.accept_location(location("c")), Action::None);
    assert_eq!(app.selected().unwrap().path, "src/c.rs");

    assert_eq!(
        app.update(Message::Key(Key::PreviousLocation)),
        Action::None
    );
    assert_eq!(app.selected().unwrap().path, "src/b.rs");
    assert_eq!(
        app.update(Message::Key(Key::PreviousLocation)),
        Action::None
    );
    assert_eq!(app.selected().unwrap().path, "src/a.rs");
    assert_eq!(app.update(Message::Key(Key::NextLocation)), Action::None);
    assert_eq!(app.selected().unwrap().path, "src/b.rs");

    assert_eq!(
        app.update(Message::Key(Key::PreviousLocation)),
        Action::None
    );
    assert_eq!(app.accept_location(location("a")), Action::None);
    assert_eq!(app.update(Message::Key(Key::NextLocation)), Action::None);
    assert_eq!(app.selected().unwrap().path, "src/b.rs");

    assert_eq!(app.accept_location(location("a")), Action::None);
    assert_eq!(app.update(Message::Key(Key::NextLocation)), Action::None);
    assert_eq!(app.selected().unwrap().path, "src/a.rs");
    assert_eq!(
        app.update(Message::Key(Key::PreviousLocation)),
        Action::None
    );
    assert_eq!(app.selected().unwrap().path, "src/b.rs");
    assert_eq!(
        app.update(Message::Key(Key::PreviousLocation)),
        Action::None
    );
    assert_eq!(app.selected().unwrap().path, "src/c.rs");
}

#[test]
fn location_history_skips_reviewed_files() {
    let mut app = location_history_app(["a", "b", "c"], |name| {
        vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: format!(" fn {name}() {{}}"),
        }]
    });
    let location = |name| source_location(&format!("/repo/src/{name}.rs"), 0);
    assert_eq!(app.accept_location(location("b")), Action::None);
    assert_eq!(app.accept_location(location("c")), Action::None);
    app.files[1].status = ReviewStatus::Reviewed;

    assert_eq!(
        app.update(Message::Key(Key::PreviousLocation)),
        Action::None
    );
    assert_eq!(app.selected().unwrap().path, "src/a.rs");
    assert_eq!(app.update(Message::Key(Key::NextLocation)), Action::None);
    assert_eq!(app.selected().unwrap().path, "src/c.rs");
}

#[test]
fn external_jump_does_not_record_a_reviewed_origin() {
    let mut app = location_history_app(["a"], |_| {
        vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: " reviewed".to_owned(),
        }]
    });
    app.files[0].status = ReviewStatus::Reviewed;
    let external = source_location("/outside.rs", 0);
    assert_eq!(
        app.accept_location(external.clone()),
        Action::LoadSource {
            snapshot_id: "commit".to_owned(),
            location: external.clone(),
            mode: SourceLoadMode::External,
        }
    );
    assert_eq!(
        app.update(Message::SourceLoaded {
            snapshot_id: "commit".to_owned(),
            location: external,
            content: b"outside\n".to_vec(),
            mode: SourceLoadMode::External,
        }),
        Action::None
    );
    assert!(app.selected().unwrap().temporary);

    assert_eq!(
        app.update(Message::Key(Key::PreviousLocation)),
        Action::None
    );
    assert!(app.selected().unwrap().temporary);
}

#[test]
fn location_history_skips_a_source_that_becomes_a_reviewed_file() {
    let mut app = location_history_app(["a"], |_| {
        vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: " current".to_owned(),
        }]
    });
    let future_file = source_location("/repo/src/future.rs", 0);
    assert!(matches!(
        app.accept_location(future_file.clone()),
        Action::LoadSource { .. }
    ));
    assert_eq!(
        app.update(Message::SourceLoaded {
            snapshot_id: "commit".to_owned(),
            location: future_file,
            content: b"future\n".to_vec(),
            mode: SourceLoadMode::External,
        }),
        Action::None
    );
    assert_eq!(
        app.accept_location(source_location("/repo/src/a.rs", 0)),
        Action::None
    );
    app.files
        .push(ReviewFile::new("src/future.rs", ReviewStatus::Reviewed));
    app.rebuild_file_tree();

    assert_eq!(
        app.update(Message::Key(Key::PreviousLocation)),
        Action::None
    );
    assert_eq!(app.selected().unwrap().path, "src/a.rs");
}

#[test]
fn location_history_includes_file_cursor_and_search_jumps() {
    let mut app = location_history_app(["a", "b"], |name| {
        (1..=3)
            .map(|line| DiffRow::Context {
                old_line: line,
                new_line: line,
                text: if name == "b" && line == 2 {
                    " needle".to_owned()
                } else {
                    format!(" line {line}")
                },
            })
            .collect()
    });

    assert_eq!(app.update(Message::Key(Key::Down)), Action::None);
    assert_eq!(app.selected().unwrap().path, "src/b.rs");
    assert_eq!(app.update(Message::Key(Key::Tab)), Action::None);
    assert_eq!(app.update(Message::Key(Key::Last)), Action::None);
    assert_eq!(app.selected().unwrap().cursor, 2);
    assert_eq!(app.update(Message::Key(Key::Up)), Action::None);
    assert_eq!(app.selected().unwrap().cursor, 1);

    assert_eq!(
        app.update(Message::Key(Key::PreviousLocation)),
        Action::None
    );
    assert_eq!(
        (
            app.selected().unwrap().path.as_str(),
            app.selected().unwrap().cursor
        ),
        ("src/b.rs", 0)
    );
    assert_eq!(
        app.update(Message::Key(Key::PreviousLocation)),
        Action::None
    );
    assert_eq!(app.selected().unwrap().path, "src/a.rs");
    assert_eq!(app.update(Message::Key(Key::NextLocation)), Action::None);
    assert_eq!(app.selected().unwrap().path, "src/b.rs");

    assert_eq!(app.update(Message::Key(Key::Char('/'))), Action::None);
    for character in "needle".chars() {
        assert_eq!(app.update(Message::Key(Key::Char(character))), Action::None);
    }
    assert_eq!(app.update(Message::Key(Key::Enter)), Action::None);
    assert_eq!(app.selected().unwrap().cursor, 1);
    assert_eq!(
        app.update(Message::Key(Key::PreviousLocation)),
        Action::None
    );
    assert_eq!(app.selected().unwrap().cursor, 0);
}

#[test]
fn jumps_center_the_cursor_when_the_viewport_has_space() {
    let mut app = location_history_app(["a"], |_| {
        (1..=40)
            .map(|line| DiffRow::Context {
                old_line: line,
                new_line: line,
                text: format!(" line {line}"),
            })
            .collect()
    });
    assert_eq!(app.update(Message::Key(Key::Tab)), Action::None);
    assert_eq!(app.update(Message::Key(Key::Char('/'))), Action::None);
    for character in "line 20".chars() {
        assert_eq!(app.update(Message::Key(Key::Char(character))), Action::None);
    }
    assert_eq!(app.update(Message::Key(Key::Enter)), Action::None);

    let file = app.selected().unwrap();
    assert_eq!(file.cursor - file.scroll, app.page_rows() / 2);
}

#[test]
fn abandoned_search_restores_its_origin_without_recording_a_jump() {
    let mut app = location_history_app(["a"], |_| {
        (1..=20)
            .map(|line| DiffRow::Context {
                old_line: line,
                new_line: line,
                text: format!(" line {line}"),
            })
            .collect()
    });
    assert_eq!(app.update(Message::Key(Key::Tab)), Action::None);
    assert_eq!(app.update(Message::Key(Key::Char('/'))), Action::None);
    for character in "line 10".chars() {
        assert_eq!(app.update(Message::Key(Key::Char(character))), Action::None);
    }
    assert_eq!(app.selected().unwrap().cursor, 9);

    assert_eq!(app.update(Message::Key(Key::Escape)), Action::None);
    assert_eq!(app.selected().unwrap().cursor, 0);
    assert_eq!(
        app.update(Message::Key(Key::PreviousLocation)),
        Action::None
    );
    assert_eq!(app.selected().unwrap().cursor, 0);
}

fn location_history_app<const FILE_COUNT: usize>(
    names: [&str; FILE_COUNT],
    rows: impl Fn(&str) -> Vec<DiffRow>,
) -> ReviewApp {
    let mut app = ReviewApp::new(
        crate::Theme::default(),
        None,
        review_store::OutputTarget::default(),
        PathBuf::from("/repo"),
    );
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: names
            .map(|name| ReviewFile::new(format!("src/{name}.rs"), ReviewStatus::Unreviewed))
            .to_vec(),
    });
    for name in names {
        app.update(Message::DiffLoaded {
            commit_id: "commit".to_owned(),
            path: format!("src/{name}.rs"),
            rows: rows(name),
            old_content: None,
            new_content: None,
        });
    }
    app
}

#[test]
fn current_cursor_becomes_external_when_its_file_leaves_the_diff() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "first".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "first".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: " fn target() {}".to_owned(),
        }],
        old_content: None,
        new_content: None,
    });
    app.collapsed_directories.insert("src".to_owned());
    app.rebuild_file_tree();
    let location = source_location("src/lib.rs", 0);

    assert_eq!(app.accept_location(location.clone()), Action::None);
    assert!(!app.collapsed_directories.contains("src"));
    let cursor = SourceLocation {
        end_byte_column: location.byte_column,
        ..location.clone()
    };
    assert_eq!(
        app.update(Message::FilesLoaded {
            change_id: "change".to_owned(),
            commit_id: "second".to_owned(),
            description: String::new(),
            files: vec![ReviewFile::new("README.md", ReviewStatus::Unreviewed)],
        }),
        Action::LoadSource {
            snapshot_id: "second".to_owned(),
            location: cursor.clone(),
            mode: SourceLoadMode::External,
        }
    );
    app.update(Message::SourceLoaded {
        snapshot_id: "second".to_owned(),
        location: cursor,
        content: b"fn target() {}\n".to_vec(),
        mode: SourceLoadMode::External,
    });
    app.collapsed_directories.insert("src".to_owned());
    assert_eq!(
        app.update(Message::FilesLoaded {
            change_id: "change".to_owned(),
            commit_id: "third".to_owned(),
            description: String::new(),
            files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
        }),
        Action::LoadDiff {
            commit_id: "third".to_owned(),
            path: "src/lib.rs".to_owned(),
        }
    );
    assert!(!app.collapsed_directories.contains("src"));
}

#[test]
fn current_cursor_is_rendered_from_the_refreshed_diff() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "first".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "first".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: " fn target() {}".to_owned(),
        }],
        old_content: None,
        new_content: None,
    });
    let location = source_location("src/lib.rs", 0);
    assert_eq!(app.accept_location(location), Action::None);
    assert_eq!(
        app.update(Message::FilesLoaded {
            change_id: "change".to_owned(),
            commit_id: "second".to_owned(),
            description: String::new(),
            files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
        }),
        Action::LoadDiff {
            commit_id: "second".to_owned(),
            path: "src/lib.rs".to_owned(),
        }
    );
    assert_eq!(
        app.update(Message::DiffLoaded {
            commit_id: "second".to_owned(),
            path: "src/lib.rs".to_owned(),
            rows: vec![DiffRow::Context {
                old_line: 2,
                new_line: 2,
                text: " another_line();".to_owned(),
            }],
            old_content: None,
            new_content: Some(b"fn target() {}\nanother_line();\n".to_vec()),
        }),
        Action::None
    );
    assert_eq!(app.current_source(), Some((0, "fn target() {}".to_owned())));
}

#[test]
fn rust_analyzer_initialization_uses_a_long_toast() {
    let mut app = ReviewApp::default();

    app.update(Message::Lsp(Event::Initializing));
    assert!(app.lsp_initialization_toast.is_some());

    app.update(Message::Lsp(Event::Ready));
    assert!(app.lsp_initialization_toast.is_none());

    app.update(Message::Lsp(Event::Initializing));
    app.update(Message::Lsp(Event::Failed {
        toast_id: None,
        snapshot_id: None,
        message: "rust-analyzer failed".to_owned(),
    }));
    assert!(app.lsp_initialization_toast.is_none());
}

#[test]
fn stale_rust_analyzer_failure_is_hidden() {
    let mut app = ReviewApp::default();
    app.update(Message::Lsp(Event::Failed {
        toast_id: Some(ToastId::generate()),
        snapshot_id: Some("old review".to_owned()),
        message: "stale LSP failure".to_owned(),
    }));
    let mut buffer = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 80, 12));

    app.toasts
        .render(buffer.area, &mut buffer, Color::Green, Color::Red);

    assert!(
        !buffer
            .content
            .iter()
            .any(|cell| cell.symbol().contains("stale"))
    );
}

#[test]
fn context_menu_hit_area_is_clamped_to_the_terminal() {
    let menu = ContextMenu {
        column: 79,
        row: 23,
        selected: 0,
        enabled: true,
    };

    assert_eq!(menu.area(Rect::new(0, 0, 80, 24)), Rect::new(58, 19, 22, 5));
}

#[test]
fn context_menu_keys_move_within_bounds_and_escape() {
    let mut app = ReviewApp {
        context_menu: Some(ContextMenu {
            column: 10,
            row: 5,
            selected: 0,
            enabled: false,
        }),
        ..ReviewApp::default()
    };

    app.update(Message::Key(Key::Up));
    assert_eq!(app.context_menu.as_ref().unwrap().selected, 0);
    for _ in 0..4 {
        app.update(Message::Key(Key::Down));
    }
    assert_eq!(app.context_menu.as_ref().unwrap().selected, 2);
    assert_eq!(app.update(Message::Key(Key::Enter)), Action::None);
    assert!(app.context_menu.is_none());

    app.context_menu = Some(ContextMenu {
        column: 10,
        row: 5,
        selected: 1,
        enabled: false,
    });
    app.update(Message::Key(Key::Escape));
    assert!(app.context_menu.is_none());
}

fn app_with_location_results() -> (ReviewApp, SourceLocation, SourceLocation, String) {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    let first = source_location("src/first.rs", 20);
    let second = source_location("src/second.rs", 38);
    let content = numbered_lines(40);
    assert_eq!(
        app.update(Message::Lsp(Event::Locations {
            toast_id: ToastId::generate(),
            operation: Operation::References,
            snapshot_id: "commit".to_owned(),
            locations: vec![first.clone(), second.clone()],
        })),
        Action::LoadSource {
            snapshot_id: "commit".to_owned(),
            location: first.clone(),
            mode: SourceLoadMode::Preview,
        }
    );
    (app, first, second, content)
}

#[test]
fn location_results_preview_disk_sources() {
    let (mut app, first, second, content) = app_with_location_results();
    assert_eq!(
        app.update(Message::SourceLoaded {
            snapshot_id: "commit".to_owned(),
            location: first.clone(),
            content: content.as_bytes().to_vec(),
            mode: SourceLoadMode::Preview,
        }),
        Action::None
    );
    let preview = app.displayed().unwrap();
    assert!((preview.scroll..preview.scroll + app.page_rows()).contains(&preview.cursor));
    assert_source_location_highlighted(&app);
    assert_eq!(
        app.update(Message::Key(Key::Last)),
        Action::LoadSource {
            snapshot_id: "commit".to_owned(),
            location: second.clone(),
            mode: SourceLoadMode::Preview,
        }
    );
    assert_eq!(
        app.update(Message::Key(Key::First)),
        Action::LoadSource {
            snapshot_id: "commit".to_owned(),
            location: first.clone(),
            mode: SourceLoadMode::Preview,
        }
    );
    assert_eq!(
        app.update(Message::Key(Key::Down)),
        Action::LoadSource {
            snapshot_id: "commit".to_owned(),
            location: second.clone(),
            mode: SourceLoadMode::Preview,
        }
    );
    assert_eq!(
        app.preview.as_ref().unwrap().disk_path.as_deref(),
        Some(first.path.as_path())
    );
    app.update(Message::SourceLoaded {
        snapshot_id: "commit".to_owned(),
        location: first.clone(),
        content: content.as_bytes().to_vec(),
        mode: SourceLoadMode::Preview,
    });
    assert_eq!(
        app.preview.as_ref().unwrap().disk_path.as_deref(),
        Some(first.path.as_path())
    );
    app.update(Message::SourceLoaded {
        snapshot_id: "commit".to_owned(),
        location: second.clone(),
        content: content.as_bytes().to_vec(),
        mode: SourceLoadMode::Preview,
    });
    assert_eq!(
        app.preview.as_ref().unwrap().disk_path.as_deref(),
        Some(second.path.as_path())
    );
    let preview = app.preview.as_ref().unwrap();
    assert!((preview.scroll..preview.scroll + app.page_rows()).contains(&preview.cursor));
}

#[test]
fn location_results_accept_a_disk_source() {
    let (mut app, first, _second, content) = app_with_location_results();
    app.update(Message::SourceLoaded {
        snapshot_id: "commit".to_owned(),
        location: first.clone(),
        content: content.as_bytes().to_vec(),
        mode: SourceLoadMode::Preview,
    });
    assert_eq!(
        app.update(Message::Key(Key::Up)),
        Action::LoadSource {
            snapshot_id: "commit".to_owned(),
            location: first.clone(),
            mode: SourceLoadMode::Preview,
        }
    );
    app.update(Message::SourceLoaded {
        snapshot_id: "commit".to_owned(),
        location: first.clone(),
        content: content.as_bytes().to_vec(),
        mode: SourceLoadMode::Preview,
    });
    assert_eq!(
        app.update(Message::Key(Key::Enter)),
        Action::LoadSource {
            snapshot_id: "commit".to_owned(),
            location: first.clone(),
            mode: SourceLoadMode::External,
        }
    );
    app.update(Message::SourceLoaded {
        snapshot_id: "commit".to_owned(),
        location: first,
        content: content.into_bytes(),
        mode: SourceLoadMode::External,
    });
    assert!(app.selected().unwrap().temporary);
    assert_eq!(app.selected().unwrap().cursor, 20);
}

#[test]
fn review_location_preview_keeps_diff_markers_and_escape_restores_diff_focus() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![
            ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed),
            ReviewFile::new("src/changed.rs", ReviewStatus::Unreviewed),
        ],
    });
    let changed = source_location("src/changed.rs", 0);
    assert_eq!(
        app.update(Message::Lsp(Event::Locations {
            toast_id: ToastId::generate(),
            operation: Operation::References,
            snapshot_id: "commit".to_owned(),
            locations: vec![changed.clone(), source_location("src/external.rs", 0)],
        })),
        Action::LoadDiff {
            commit_id: "commit".to_owned(),
            path: "src/changed.rs".to_owned(),
        }
    );
    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/changed.rs".to_owned(),
        rows: vec![DiffRow::Add {
            new_line: 1,
            text: "+fn changed() {}".to_owned(),
        }],
        old_content: None,
        new_content: Some(b"fn changed() {}\n".to_vec()),
    });

    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.view(), frame.area()))
        .unwrap();
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| { cell.symbol() == "▌" && cell.fg == app.palette.insertion })
    );

    app.update(Message::Key(Key::Escape));
    assert_eq!(app.selected_file, 0);
    assert_eq!(app.focus, Focus::Diff);
    assert!(app.locations.is_none());
    assert!(app.preview.is_none());
}

#[test]
fn wrapped_location_preview_keeps_the_target_visible() {
    let mut app = ReviewApp::default();
    app.update(Message::Resize {
        width: 80,
        height: 8,
    });
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    let source = format!("start-{}-visible-tail\n", "middle".repeat(30));
    let mut target = source_location("src/first.rs", 0);
    target.byte_column = source.trim_end().len();
    target.end_byte_column = target.byte_column;
    let other = source_location("src/second.rs", 0);
    app.update(Message::Lsp(Event::Locations {
        toast_id: ToastId::generate(),
        operation: Operation::References,
        snapshot_id: "commit".to_owned(),
        locations: vec![target.clone(), other],
    }));
    app.update(Message::SourceLoaded {
        snapshot_id: "commit".to_owned(),
        location: target,
        content: source.into_bytes(),
        mode: SourceLoadMode::Preview,
    });

    let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.view(), frame.area()))
        .unwrap();
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("visible-tail"), "{rendered:?}");
    let hidden_cursor = app.selected().unwrap().cursor;
    app.update(Message::MouseClick {
        column: 65,
        row: 1,
        insert_path: false,
    });
    app.update(Message::MouseClick {
        column: 50,
        row: 3,
        insert_path: false,
    });
    app.update(Message::MouseRightClick { column: 50, row: 3 });
    assert_eq!(app.selected().unwrap().cursor, hidden_cursor);
    assert_eq!(app.focus, Focus::Files);
    assert!(app.context_menu.is_none());
}

#[test]
fn reviewed_file_reference_preview_shows_each_target() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/navigation.rs", ReviewStatus::Reviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/navigation.rs".to_owned(),
        rows: (0..30)
            .map(|line| DiffRow::Context {
                old_line: line + 1,
                new_line: line + 1,
                text: format!(" target_{line}"),
            })
            .collect(),
        old_content: None,
        new_content: None,
    });
    let first = source_location("src/navigation.rs", 5);
    let second = source_location("src/navigation.rs", 20);
    app.update(Message::Lsp(Event::Locations {
        toast_id: ToastId::generate(),
        operation: Operation::References,
        snapshot_id: "commit".to_owned(),
        locations: vec![first, second],
    }));

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let screen = |terminal: &Terminal<TestBackend>| {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .fold(String::new(), |mut text, cell| {
                text.push_str(cell.symbol());
                text
            })
    };
    terminal
        .draw(|frame| frame.render_widget(app.view(), frame.area()))
        .unwrap();
    assert!(screen(&terminal).contains("target_5"));

    app.update(Message::Key(Key::Down));
    terminal
        .draw(|frame| frame.render_widget(app.view(), frame.area()))
        .unwrap();
    assert!(screen(&terminal).contains("target_20"));
}

#[test]
fn reviewed_lsp_target_hides_after_visiting_another_file() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![
            ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed),
            ReviewFile::new("src/other.rs", ReviewStatus::Reviewed),
        ],
    });
    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: " fn target() {}".to_owned(),
        }],
        old_content: None,
        new_content: None,
    });
    app.accept_location(source_location("src/lib.rs", 0));

    assert!(matches!(
        app.update(Message::Key(Key::Space)),
        Action::SetReviewed { reviewed: true, .. }
    ));
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.view(), frame.area()))
        .unwrap();
    let rendered = |terminal: &Terminal<TestBackend>| {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>()
    };
    assert!(rendered(&terminal).contains("fn target() {}"));

    app.update(Message::Key(Key::Tab));
    app.update(Message::Key(Key::Down));
    app.update(Message::Key(Key::Up));
    terminal
        .draw(|frame| frame.render_widget(app.view(), frame.area()))
        .unwrap();
    assert!(rendered(&terminal).contains("No changes"));
}

#[test]
fn empty_review_diff_previews_its_full_source_after_loading() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![
            ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed),
            ReviewFile::new("tests/ui_state.rs", ReviewStatus::Unreviewed),
        ],
    });
    let location = source_location("tests/ui_state.rs", 20);
    assert_eq!(
        app.update(Message::Lsp(Event::Locations {
            toast_id: ToastId::generate(),
            operation: Operation::References,
            snapshot_id: "commit".to_owned(),
            locations: vec![location.clone(), source_location("src/tests.rs", 0)],
        })),
        Action::LoadDiff {
            commit_id: "commit".to_owned(),
            path: "tests/ui_state.rs".to_owned(),
        }
    );

    assert_eq!(
        app.update(Message::DiffLoaded {
            commit_id: "commit".to_owned(),
            path: "tests/ui_state.rs".to_owned(),
            rows: Vec::new(),
            old_content: None,
            new_content: Some(numbered_lines(30).into_bytes()),
        }),
        Action::None
    );
    assert_eq!(app.displayed().unwrap().cursor, 20);
}

fn source_location(path: &str, line: u32) -> SourceLocation {
    SourceLocation {
        path: PathBuf::from(path),
        line,
        byte_column: 0,
        end_line: line,
        end_byte_column: 1,
    }
}

fn numbered_lines(count: usize) -> String {
    (0..count).fold(String::new(), |mut content, line| {
        writeln!(content, "line {line}").unwrap();
        content
    })
}

fn search_test_app() -> ReviewApp {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![
            ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed),
            ReviewFile::new("src/other.rs", ReviewStatus::Reviewed),
            ReviewFile::new("src/unopened.rs", ReviewStatus::Unreviewed),
        ],
    });
    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: [
            "start",
            "noise",
            "filler",
            "Needle one",
            "more",
            "needle two",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, text)| DiffRow::Context {
            old_line: u32::try_from(index + 1).unwrap(),
            new_line: u32::try_from(index + 1).unwrap(),
            text: format!(" {text}"),
        })
        .collect(),
        old_content: None,
        new_content: None,
    });
    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/other.rs".to_owned(),
        rows: vec![
            DiffRow::Context {
                old_line: 1,
                new_line: 1,
                text: " no match".to_owned(),
            },
            DiffRow::Context {
                old_line: 2,
                new_line: 2,
                text: " needle in another file".to_owned(),
            },
        ],
        old_content: None,
        new_content: None,
    });
    app
}

fn assert_search_highlighted(terminal: &mut Terminal<TestBackend>, app: &ReviewApp) {
    terminal
        .draw(|frame| frame.render_widget(app.view(), frame.area()))
        .unwrap();
    assert!((0..12).any(|row| {
        (0..80).any(|column| {
            let cell = &terminal.backend().buffer()[(column, row)];
            cell.modifier.contains(ratatui::style::Modifier::REVERSED)
                && cell.fg != app.palette.deletion
                && cell.bg != app.palette.warning
        })
    }));
}

fn assert_source_location_highlighted(app: &ReviewApp) {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.view(), frame.area()))
        .unwrap();
    assert!(terminal.backend().buffer().content().iter().any(|cell| {
        cell.modifier.contains(ratatui::style::Modifier::REVERSED)
            && cell.fg != app.palette.deletion
            && cell.bg != app.palette.warning
    }));
}
