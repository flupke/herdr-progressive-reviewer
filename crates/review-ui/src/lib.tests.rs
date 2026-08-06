use std::fmt::Write;
use std::path::{Path, PathBuf};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use review_lsp::{Event, Operation, SourceLocation};
use review_repository::diff::DiffRow;
use toasts::ToastId;

use crate::app::{Action, ContextMenu, Focus, Key, Message, ReviewApp, ReviewFile, SourceLoadMode};
use review_state::ReviewStatus;

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
}

#[test]
fn lsp_keys_use_the_visible_current_source() {
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

    app.files[0].column = 0;
    app.move_word(true);
    assert_eq!(app.files[0].column, 3);
    app.files[0].column = 5;
    app.move_word(false);
    assert_eq!(app.files[0].column, 3);

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
fn location_results_preview_and_accept_disk_source() {
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
    assert_eq!(preview.cursor - preview.scroll, app.page_rows() / 2);
    assert_source_location_highlighted(&app);
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
    assert_eq!(preview.cursor - preview.scroll, app.page_rows() / 2);
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
