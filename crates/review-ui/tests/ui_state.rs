use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use review_repository::{
    diff::{DiffRow, NoticeKind},
    repository::DiffStatistics,
};
use review_source::ReviewCheckpoint;
use review_state::{ReviewState, ReviewStatus};
use review_ui::{
    Action, DocumentAction, DocumentLoad, Key, LspAction, RepositoryAction, ReviewApplication,
    SettingsAction, TerminalAction, UserInput,
};
use std::time::Instant;
use ui_events::{
    AnimationTick, FileSummary, RepositoryFilesChanged, RepositoryMetadataChanged,
    ReviewStateSaved, ToastExpirationTick,
};

fn rows() -> Vec<DiffRow> {
    vec![
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
    ]
}

fn screen(app: &ReviewApplication, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.frame(), frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buffer[(x, y)].symbol());
            }
            line
        })
        .collect()
}

/// Whether the pane whose bottom border crosses `column` has focus, which
/// its border shows in the accent color.
fn pane_has_focus(app: &ReviewApplication, width: u16, height: u16, column: u16) -> bool {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.frame(), frame.area()))
        .unwrap();
    terminal.backend().buffer()[(column, height - 2)].fg
        == review_ui::Theme::default().palette.focus
}

fn application_screen(app: &ReviewApplication, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.frame(), frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buffer[(x, y)].symbol());
            }
            line
        })
        .collect()
}

fn click_on_text(screen: &[String], text: &str) -> UserInput {
    let (row, column) = screen
        .iter()
        .enumerate()
        .find_map(|(row, line)| {
            line.find(text).map(|column| {
                (
                    u16::try_from(row).unwrap(),
                    u16::try_from(line[..column].chars().count()).unwrap(),
                )
            })
        })
        .unwrap_or_else(|| panic!("{text:?} must be visible in {screen:?}"));
    UserInput::MouseClick { column, row }
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

fn wrapped_diff_application(rows: Vec<DiffRow>, width: u16, height: u16) -> ReviewApplication {
    let mut application = ReviewApplication::default();
    publish_repository(
        &mut application,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    application.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: "src/lib.rs".to_owned(),
        rows,
        old_content: None,
        new_content: None,
        hunks: review_hunks::FileHunks::default(),
    });
    application.update(UserInput::Resize { width, height });
    application.update(UserInput::Key(Key::Tab));
    application
}

#[test]
fn gy_queries_the_cursor_type_and_jumps_to_a_single_definition() {
    let source = "let value = Thing;";
    let mut app = wrapped_diff_application(
        vec![
            DiffRow::Context {
                old_line: 1,
                new_line: 1,
                text: " struct Thing;".to_owned(),
            },
            DiffRow::Context {
                old_line: 2,
                new_line: 2,
                text: format!(" {source}"),
            },
        ],
        80,
        12,
    );
    app.update(UserInput::Key(Key::Down));
    app.update(UserInput::Key(Key::Char('w')));
    assert!(app.update(UserInput::Key(Key::Char('g'))).is_empty());
    let actions = app.update(UserInput::Key(Key::Char('y')));
    let [Action::Lsp(LspAction::Request { operation, query })] = actions.as_slice() else {
        panic!("gy must query the type definition");
    };
    assert_eq!(*operation, review_lsp::Operation::TypeDefinition);
    assert_eq!((query.line, query.byte_column), (1, 4));
    assert_eq!(query.expected_line, source);

    app.publish(review_lsp::Event::Locations {
        toast_id: query.toast_id,
        operation: *operation,
        snapshot_id: query.snapshot_id.clone(),
        locations: vec![review_lsp::SourceLocation {
            path: query.path.clone(),
            line: 0,
            byte_column: 7,
            end_line: 0,
            end_byte_column: 12,
        }],
    });
    let actions = app.update(UserInput::Key(Key::Char('K')));
    let [Action::Lsp(LspAction::Request { query, .. })] = actions.as_slice() else {
        panic!("a single type definition must jump directly to the source");
    };
    assert_eq!((query.line, query.byte_column), (0, 7));
    assert_eq!(query.expected_line, "struct Thing;");
}

#[test]
fn shift_e_opens_the_current_file_at_the_cursor_line_from_either_pane() {
    let mut app = wrapped_diff_application(
        vec![
            DiffRow::Context {
                old_line: 1,
                new_line: 1,
                text: " first".to_owned(),
            },
            DiffRow::Context {
                old_line: 2,
                new_line: 2,
                text: " second".to_owned(),
            },
        ],
        80,
        12,
    );
    app.update(UserInput::Key(Key::Down));
    for _ in 0..2 {
        let actions = app.update(UserInput::Key(Key::Char('E')));
        let [Action::Terminal(TerminalAction::OpenInEditor { path, line })] = actions.as_slice()
        else {
            panic!("E must open the current file, got {actions:?}");
        };
        assert!(path.ends_with("src/lib.rs"));
        assert_eq!(*line, Some(1));
        app.update(UserInput::Key(Key::Tab));
    }
}

#[test]
fn long_diff_lines_wrap_without_horizontal_scrolling() {
    let source = format!("start-{}-visible-tail", "middle".repeat(30));
    let mut app = wrapped_diff_application(
        vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: format!(" {source}"),
        }],
        40,
        8,
    );

    let rendered = screen(&app, 40, 8);
    assert!(rendered.join("\n").contains("start-"));
    assert!(rendered[3].starts_with("│    "));
    app.update(UserInput::Key(Key::Char('$')));

    assert!(screen(&app, 40, 8).join("\n").contains("visible-tail"));
}

#[test]
fn wrapped_continuation_mouse_targets_its_source_position() {
    let source = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut app = wrapped_diff_application(
        vec![
            DiffRow::Context {
                old_line: 1,
                new_line: 1,
                text: format!(" {source}"),
            },
            DiffRow::Context {
                old_line: 2,
                new_line: 2,
                text: " second logical line".to_owned(),
            },
        ],
        40,
        8,
    );
    app.update(UserInput::MouseClick { column: 10, row: 3 });

    let actions = app.update(UserInput::Key(Key::Char('K')));
    let [Action::Lsp(LspAction::Request { query, .. })] = actions.as_slice() else {
        panic!("wrapped source position must support LSP navigation");
    };
    assert_eq!(query.line, 0);
    assert_eq!(query.byte_column, 39);
}

#[test]
fn mouse_wheel_scrolls_through_wrapped_continuations() {
    let source = format!("first-visible-{}-last-visible", "middle".repeat(26));
    let mut app = wrapped_diff_application(
        vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: format!(" {source}"),
        }],
        40,
        6,
    );
    assert!(screen(&app, 40, 6).join("\n").contains("first-visible"));

    app.update(UserInput::MouseScroll {
        column: 6,
        row: 2,
        delta: 100,
    });

    let rendered = screen(&app, 40, 6).join("\n");
    assert!(!rendered.contains("first-visible"));
    assert!(rendered.contains("last-visible"));
}

#[test]
fn half_page_keys_move_through_wrapped_continuations() {
    let source = format!("first-visible-{}-last-visible", "middle".repeat(30));
    let mut app = wrapped_diff_application(
        vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: format!(" {source}"),
        }],
        40,
        6,
    );

    app.update(UserInput::Key(Key::HalfPageDown));
    app.update(UserInput::Key(Key::HalfPageDown));

    let rendered = screen(&app, 40, 6).join("\n");
    assert!(!rendered.contains("first-visible"));
    app.update(UserInput::Key(Key::HalfPageUp));
    app.update(UserInput::Key(Key::HalfPageUp));
    assert!(screen(&app, 40, 6).join("\n").contains("first-visible"));
}

#[test]
fn wrapped_grapheme_mouse_position_uses_its_terminal_width() {
    let joined_emoji = "👨‍👩‍👧‍👦";
    let source = format!("{}{joined_emoji}tail", "a".repeat(34));
    let mut app = wrapped_diff_application(
        vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: format!(" {source}"),
        }],
        40,
        8,
    );
    app.update(UserInput::MouseClick { column: 7, row: 3 });

    let actions = app.update(UserInput::Key(Key::Char('K')));
    let [Action::Lsp(LspAction::Request { query, .. })] = actions.as_slice() else {
        panic!("wrapped grapheme position must support LSP navigation");
    };
    assert_eq!(query.byte_column, 34 + joined_emoji.len());
}

#[test]
fn stale_diffs_are_ignored_and_selected_text_is_not_sent_on_enter() {
    let mut app = ReviewApplication::default();
    assert_eq!(
        publish_repository(
            &mut app,
            ReviewCheckpoint::new("qpvuntsm", "11111111"),
            "Commit title\n\nCommit body\n".to_owned(),
            vec![
                FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed),
                FileSummary::new("README.md", ReviewStatus::Reviewed),
            ]
        ),
        vec![
            Action::Thread(review_threads::ThreadCommand::Load("qpvuntsm".into())),
            Action::Document(DocumentAction::Load(DocumentLoad::Diff {
                review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
                path: "src/lib.rs".to_owned(),
            })),
            Action::Lsp(LspAction::OpenDocument("src/lib.rs".into()))
        ]
    );
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "stale"),
        path: "src/lib.rs".to_owned(),
        rows: vec![DiffRow::Meta {
            text: "stale result".to_owned(),
        }],
        old_content: None,
        new_content: None,
        hunks: review_hunks::FileHunks::default(),
    });
    assert!(!screen(&app, 80, 12).join("\n").contains("stale result"));
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("other-change", "11111111"),
        path: "src/lib.rs".to_owned(),
        rows: vec![DiffRow::Meta {
            text: "wrong review unit".to_owned(),
        }],
        old_content: None,
        new_content: None,
        hunks: review_hunks::FileHunks::default(),
    });
    assert!(
        !screen(&app, 80, 12)
            .join("\n")
            .contains("wrong review unit")
    );
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: "src/lib.rs".to_owned(),
        rows: rows(),
        old_content: None,
        new_content: None,
        hunks: review_hunks::FileHunks::default(),
    });

    app.update(UserInput::Key(Key::Tab));
    app.update(UserInput::Key(Key::Char('V')));
    app.update(UserInput::Key(Key::Down));
    app.update(UserInput::Key(Key::Char('V')));
    assert!(app.update(UserInput::Key(Key::Enter)).is_empty());
}

#[test]
fn enter_on_a_filename_does_not_send_text() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n".to_owned(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    assert!(app.update(UserInput::Key(Key::Enter)).is_empty());
}

#[test]
fn global_search_focuses_the_diff_and_shows_the_query_and_current_match() {
    let mut application = wrapped_diff_application(
        vec![
            DiffRow::Hunk {
                old_start: 0,
                old_count: 0,
                new_start: 1,
                new_count: 3,
            },
            DiffRow::Add {
                new_line: 1,
                text: "+first needle".to_owned(),
            },
            DiffRow::Add {
                new_line: 2,
                text: "+second needle and another needle".to_owned(),
            },
            DiffRow::Add {
                new_line: 3,
                text: "+third line".to_owned(),
            },
        ],
        80,
        12,
    );

    application.update(UserInput::Key(Key::Tab));
    assert!(
        screen(&application, 80, 12)
            .join("\n")
            .contains("Files   Threads   Explore")
    );
    application.update(UserInput::Key(Key::Char('/')));
    assert!(
        screen(&application, 80, 12)
            .join("\n")
            .contains("Diff · src/lib.rs")
    );
    assert!(pane_has_focus(&application, 80, 12, 70));
    for character in "needle".chars() {
        application.update(UserInput::Key(Key::Char(character)));
    }
    let active_search_status = screen(&application, 80, 12)[11].clone();
    assert!(active_search_status.starts_with("/needle"));
    assert!(active_search_status.ends_with("[1/3]"));
    assert!(!active_search_status.contains("Output:"));

    application.publish(ui_events::ToastRequested {
        text: "Background service starting".to_owned(),
        kind: toasts::ToastKind::Info,
    });
    let notified = screen(&application, 80, 12);
    assert!(notified.join("\n").contains("Background service starting"));
    assert_eq!(notified[11], active_search_status);

    application.update(UserInput::Key(Key::Enter));
    application.update(UserInput::Key(Key::Char('n')));
    assert!(screen(&application, 80, 12)[11].ends_with("[2/3]"));
    application.update(UserInput::Key(Key::Char('n')));
    assert!(screen(&application, 80, 12)[11].ends_with("[3/3]"));

    application.update(UserInput::Key(Key::Char('/')));
    for character in "src/lib".chars() {
        application.update(UserInput::Key(Key::Char(character)));
    }
    assert!(screen(&application, 80, 12)[11].starts_with("/src/lib"));

    application.update(UserInput::Key(Key::Escape));
    assert!(screen(&application, 80, 12)[11].starts_with("? help"));
}

#[test]
fn pasting_into_diff_search_updates_the_query_and_matches() {
    let mut application = wrapped_diff_application(
        vec![
            DiffRow::Add {
                new_line: 1,
                text: "+un café".to_owned(),
            },
            DiffRow::Add {
                new_line: 2,
                text: "+Unicode cafés".to_owned(),
            },
        ],
        80,
        12,
    );

    application.update(UserInput::Key(Key::Char('/')));
    application.update(UserInput::Paste("café".to_owned()));
    let status = screen(&application, 80, 12)[11].clone();
    assert!(status.starts_with("/café"), "{status}");
    assert!(status.ends_with("[1/2]"), "{status}");

    application.update(UserInput::Key(Key::Char('s')));
    assert!(screen(&application, 80, 12)[11].ends_with("[1/1]"));

    application.update(UserInput::Key(Key::Escape));
    application.update(UserInput::Key(Key::Char('/')));
    application.update(UserInput::Paste("Unicode\n".to_owned()));
    let status = screen(&application, 80, 12)[11].clone();
    assert!(status.starts_with("/Unicode "), "{status}");
    assert!(status.ends_with("[1/1]"), "{status}");
}

#[test]
fn reviewed_file_hides_its_diff() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Reviewed)],
    );
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: "src/lib.rs".to_owned(),
        rows: rows(),
        old_content: None,
        new_content: None,
        hunks: review_hunks::FileHunks::default(),
    });

    let screen = screen(&app, 80, 12);
    assert!(screen.join("\n").contains("No changes"));
    assert!(!screen.join("\n").contains("old();"));
}

#[test]
fn commit_message_opens_and_closes_from_mouse_or_keyboard() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        Vec::new(),
    );

    let header = screen(&app, 80, 12).join("\n");
    assert!(header.contains("Commit title"));
    assert!(header.contains("+0 -0  ━━━━━━━━━━━━ 0% reviewed"));
    assert!(!header.contains("Progressive review"));
    assert!(!header.contains("change qpvuntsm"));

    app.update(UserInput::Key(Key::Char('c')));
    app.update(UserInput::Resize {
        width: 80,
        height: 12,
    });
    let popup = screen(&app, 80, 12).join("\n");
    assert!(popup.contains("Commit message"));
    assert!(popup.contains("Commit body"));

    app.update(UserInput::MouseClick { column: 40, row: 6 });
    assert!(screen(&app, 80, 12).join("\n").contains("Commit body"));

    app.update(UserInput::MouseClick { column: 0, row: 11 });
    assert!(!screen(&app, 80, 12).join("\n").contains("Commit body"));

    app.update(UserInput::MouseClick { column: 2, row: 0 });
    assert!(screen(&app, 80, 12).join("\n").contains("Commit body"));
}

#[test]
fn optimistic_selection_stays_on_the_next_file_when_review_fails() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![
            FileSummary::new("first.rs", ReviewStatus::Unreviewed),
            FileSummary::new("second.rs", ReviewStatus::Reviewed),
            FileSummary::new("third.rs", ReviewStatus::ChangedSinceReview),
            FileSummary::new("fourth.rs", ReviewStatus::Unreviewed),
        ],
    );

    assert!(matches!(
        app.update(UserInput::Key(Key::Char(' '))).as_slice(),
        [
            Action::Repository(RepositoryAction::SetReviewed { reviewed: true, .. }),
            Action::Lsp(LspAction::OpenDocument(_))
        ]
    ));
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Diff · third.rs")
    );
    assert_eq!(
        publish_repository(
            &mut app,
            ReviewCheckpoint::new("qpvuntsm", "11111111"),
            "Commit title\n\nCommit body\n".to_owned(),
            vec![
                FileSummary::new("first.rs", ReviewStatus::Unreviewed),
                FileSummary::new("second.rs", ReviewStatus::Reviewed),
                FileSummary::new("third.rs", ReviewStatus::ChangedSinceReview),
                FileSummary::new("fourth.rs", ReviewStatus::Unreviewed),
            ]
        ),
        vec![Action::Document(DocumentAction::Load(DocumentLoad::Diff {
            review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
            path: "third.rs".to_owned(),
        }))]
    );
    assert_eq!(
        app.publish(ReviewStateSaved {
            review_unit: "qpvuntsm".into(),
            path: "first.rs".to_owned(),
            result: Err(()),
        }),
        Vec::<Action>::new()
    );
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Diff · third.rs")
    );
}

#[test]
fn added_file_renders_as_plain_file_content() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![FileSummary::new("src/main.rs", ReviewStatus::Unreviewed)],
    );
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: "src/main.rs".to_owned(),
        rows: vec![
            DiffRow::FileHeader {
                old_path: None,
                new_path: None,
                text: "diff --git a/src/main.rs b/src/main.rs".to_owned(),
            },
            DiffRow::Meta {
                text: "new file mode 100644".to_owned(),
            },
            DiffRow::Meta {
                text: "--- /dev/null".to_owned(),
            },
            DiffRow::Meta {
                text: "+++ b/src/main.rs".to_owned(),
            },
            DiffRow::Hunk {
                old_start: 0,
                old_count: 0,
                new_start: 1,
                new_count: 1,
            },
            DiffRow::Add {
                new_line: 1,
                text: "+fn main() {}".to_owned(),
            },
        ],
        old_content: None,
        new_content: Some(b"fn main() {}\n".to_vec()),
        hunks: review_hunks::FileHunks::default(),
    });

    let screen = screen(&app, 80, 12).join("\n");
    assert!(screen.contains("fn main() {}"));
    assert!(!screen.contains("new file mode"));
    assert!(!screen.contains("diff --git"));
    assert!(!screen.contains("@@"));
    assert!(!screen.contains("+fn main() {}"));
    assert!(!screen.contains('▌'));
    assert!(screen.contains("1 fn main() {}"));
}

#[test]
fn deleted_file_renders_as_plain_file_content() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![FileSummary::new("src/main.rs", ReviewStatus::Unreviewed)],
    );
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: "src/main.rs".to_owned(),
        rows: vec![
            DiffRow::Meta {
                text: "deleted file mode 100644".to_owned(),
            },
            DiffRow::Hunk {
                old_start: 1,
                old_count: 1,
                new_start: 0,
                new_count: 0,
            },
            DiffRow::Delete {
                old_line: 1,
                text: "-fn main() {}".to_owned(),
            },
        ],
        old_content: Some(b"fn main() {}\n".to_vec()),
        new_content: None,
        hunks: review_hunks::FileHunks::default(),
    });

    let screen = screen(&app, 80, 12).join("\n");
    assert!(!screen.contains("deleted file mode"));
    assert!(!screen.contains("-fn main() {}"));
    assert!(!screen.contains('▌'));
    assert!(screen.contains("1 fn main() {}"));
}

#[test]
fn diff_uses_bars_line_numbers_and_expandable_gaps() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    let rows = vec![
        DiffRow::FileHeader {
            old_path: None,
            new_path: None,
            text: "diff --git a/src/lib.rs b/src/lib.rs".to_owned(),
        },
        DiffRow::Meta {
            text: "--- a/src/lib.rs".to_owned(),
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
            text: " first".to_owned(),
        },
        DiffRow::Delete {
            old_line: 2,
            text: "-old".to_owned(),
        },
        DiffRow::Add {
            new_line: 2,
            text: "+new".to_owned(),
        },
        DiffRow::Hunk {
            old_start: 6,
            old_count: 1,
            new_start: 6,
            new_count: 1,
        },
        DiffRow::Context {
            old_line: 6,
            new_line: 6,
            text: " sixth".to_owned(),
        },
    ];
    let load = |app: &mut ReviewApplication| {
        app.publish(ui_events::DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
            path: "src/lib.rs".to_owned(),
            rows: rows.clone(),
            old_content: Some(b"first\nold\nthird\nfourth\nfifth\nsixth\n".to_vec()),
            new_content: Some(b"first\nnew\nthird\nfourth\nfifth\nsixth\n".to_vec()),
            hunks: review_hunks::FileHunks::default(),
        });
    };
    load(&mut app);

    let collapsed_rows = screen(&app, 100, 14);
    let collapsed = collapsed_rows.join("\n");
    assert_eq!(collapsed.matches('▌').count(), 2);
    assert!(collapsed.contains("▌ 2 old"));
    assert!(collapsed.contains("▌ 2 new"));
    assert!(collapsed.contains("  … 3 unmodified lines"));
    assert!(!collapsed.contains("diff --git"));
    assert!(!collapsed.contains("@@"));
    let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.frame(), frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let code_column = (0..100)
        .find(|column| buffer[(*column, 1)].symbol() == "╮")
        .unwrap()
        + 2;
    assert_ne!(buffer[(code_column, 5)].bg, Color::Reset);
    assert_eq!(buffer[(code_column, 5)].bg, buffer[(98, 5)].bg);

    app.update(UserInput::Key(Key::Tab));
    for _ in 0..3 {
        app.update(UserInput::Key(Key::Down));
    }
    app.update(UserInput::Key(Key::Char('l')));
    let expanded = screen(&app, 100, 14).join("\n");
    assert!(expanded.contains("3 third"));
    assert!(!expanded.contains("… 3 unmodified lines"));

    load(&mut app);
    app.update(UserInput::MouseClick { column: 70, row: 5 });
    assert!(screen(&app, 100, 14).join("\n").contains("3 third"));
}

#[test]
fn diff_controls_expand_and_contract_all_gaps() {
    let mut app = ReviewApplication::default();
    let path = "src/a/very/long/path/that/must/leave/room/for/the/buttons/lib.rs";
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![FileSummary::new(path, ReviewStatus::Unreviewed)],
    );
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: path.to_owned(),
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
                old_start: 3,
                old_count: 1,
                new_start: 3,
                new_count: 1,
            },
            DiffRow::Context {
                old_line: 3,
                new_line: 3,
                text: " last".to_owned(),
            },
        ],
        old_content: Some(b"first\nmiddle\nlast\n".to_vec()),
        new_content: Some(b"first\nmiddle\nlast\n".to_vec()),
        hunks: review_hunks::FileHunks::default(),
    });
    app.update(UserInput::Resize {
        width: 100,
        height: 14,
    });

    let collapsed = screen(&app, 100, 14).join("\n");
    assert!(collapsed.contains("←→"));
    assert!(collapsed.contains("→←"));
    assert!(collapsed.contains('👁'));
    assert!(collapsed.contains("1 unmodified line"));
    app.update(UserInput::MouseClick { column: 87, row: 1 });
    assert!(!screen(&app, 100, 14).join("\n").contains("unmodified line"));
    app.update(UserInput::MouseClick { column: 92, row: 1 });
    assert!(
        screen(&app, 100, 14)
            .join("\n")
            .contains("1 unmodified line")
    );

    app.update(UserInput::MouseClick { column: 96, row: 1 });
    let file = screen(&app, 100, 14).join("\n");
    assert!(file.contains("File ·"));
    assert!(file.contains('✕'));
    assert!(!file.contains("←→"));
    assert!(!file.contains("→←"));
    assert!(file.contains("2 middle"));
    assert!(!file.contains("unmodified line"));

    app.update(UserInput::MouseClick { column: 97, row: 1 });
    let diff = screen(&app, 100, 14).join("\n");
    assert!(diff.contains("Diff ·"));
    assert!(diff.contains("←→"));
    assert!(diff.contains("→←"));
    assert!(diff.contains("1 unmodified line"));
}

#[test]
fn marking_a_changed_file_reviewed_replaces_its_baseline() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![FileSummary::new(
            "src/lib.rs",
            ReviewStatus::ChangedSinceReview,
        )],
    );
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: "src/lib.rs".to_owned(),
        rows: rows(),
        old_content: None,
        new_content: None,
        hunks: review_hunks::FileHunks::default(),
    });

    assert_eq!(
        app.update(UserInput::Key(Key::Char(' '))),
        vec![Action::Repository(RepositoryAction::SetReviewed {
            path: "src/lib.rs".to_owned(),
            reviewed: true,
        })]
    );
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("No changes")
    );
    assert_eq!(
        app.publish(ReviewStateSaved {
            review_unit: "qpvuntsm".into(),
            path: "src/lib.rs".to_owned(),
            result: Err(()),
        }),
        Vec::<Action>::new()
    );
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("old();")
    );

    assert!(matches!(
        app.update(UserInput::Key(Key::Char(' '))).as_slice(),
        [Action::Repository(RepositoryAction::SetReviewed {
            reviewed: true,
            ..
        })]
    ));
    assert_eq!(
        app.publish(ReviewStateSaved {
            review_unit: "qpvuntsm".into(),
            path: "src/lib.rs".to_owned(),
            result: Ok(ReviewState {
                status: ReviewStatus::Reviewed,
                warning: None,
                current_diff_statistics: DiffStatistics::default(),
                lines: None,
            }),
        }),
        Vec::<Action>::new()
    );
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("No changes")
    );
}

#[test]
fn mouse_targets_the_hovered_pane_and_click_changes_focus() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![
            FileSummary::new("first.rs", ReviewStatus::Unreviewed),
            FileSummary::new("second.rs", ReviewStatus::Unreviewed),
        ],
    );
    app.update(UserInput::Resize {
        width: 80,
        height: 12,
    });

    assert_eq!(
        app.update(UserInput::MouseScroll {
            column: 1,
            row: 2,
            delta: 1,
        }),
        [Action::Lsp(LspAction::OpenDocument("second.rs".into()))]
    );
    assert_eq!(
        publish_tick(&mut app, Instant::now()),
        vec![Action::Document(DocumentAction::Load(DocumentLoad::Diff {
            review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
            path: "second.rs".to_owned(),
        }))]
    );
    app.update(UserInput::MouseClick { column: 70, row: 2 });
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Diff · second.rs")
    );
    assert!(pane_has_focus(&app, 80, 12, 70));
    app.update(UserInput::MouseScroll {
        column: 1,
        row: 2,
        delta: -1,
    });
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Diff · first.rs")
    );
    assert!(pane_has_focus(&app, 80, 12, 70));
    app.update(UserInput::MouseClick { column: 1, row: 3 });
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Diff · second.rs")
    );
    assert!(app.update(UserInput::Key(Key::Enter)).is_empty());
    assert_eq!(
        app.update(UserInput::MouseControlClick { column: 1, row: 2 }),
        vec![Action::Lsp(LspAction::OpenDocument("first.rs".into()))]
    );
}

#[test]
fn double_clicking_a_file_marks_it_reviewed() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n".to_owned(),
        vec![
            FileSummary::new("first.rs", ReviewStatus::Unreviewed),
            FileSummary::new("second.rs", ReviewStatus::Unreviewed),
        ],
    );
    app.update(UserInput::Resize {
        width: 80,
        height: 12,
    });

    app.update(UserInput::MouseClick { column: 1, row: 2 });
    assert_eq!(
        app.update(UserInput::MouseDoubleClick { column: 1, row: 2 }),
        vec![
            Action::Repository(RepositoryAction::SetReviewed {
                path: "first.rs".to_owned(),
                reviewed: true,
            }),
            Action::Lsp(LspAction::OpenDocument("second.rs".into()))
        ]
    );
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Diff · second.rs")
    );
}

#[test]
fn double_clicking_a_reviewed_file_marks_it_unreviewed() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n".to_owned(),
        vec![FileSummary::new("reviewed.rs", ReviewStatus::Reviewed)],
    );

    app.update(UserInput::MouseClick { column: 1, row: 2 });
    assert_eq!(
        app.update(UserInput::MouseDoubleClick { column: 1, row: 2 }),
        vec![
            Action::Repository(RepositoryAction::SetReviewed {
                path: "reviewed.rs".to_owned(),
                reviewed: false,
            }),
            Action::Document(DocumentAction::Load(DocumentLoad::Diff {
                review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
                path: "reviewed.rs".to_owned(),
            })),
        ]
    );
}

#[test]
fn clicking_a_directory_collapses_its_descendants_across_refreshes() {
    let mut app = ReviewApplication::default();
    let files = || {
        vec![
            FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed),
            FileSummary::new("src/main.rs", ReviewStatus::Unreviewed),
            FileSummary::new("tests/test.rs", ReviewStatus::Unreviewed),
        ]
    };
    app.update(UserInput::Resize {
        width: 80,
        height: 12,
    });
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title".to_owned(),
        files(),
    );

    app.update(UserInput::MouseClick { column: 4, row: 2 });
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("lib.rs")
    );

    assert_eq!(
        app.update(UserInput::MouseClick { column: 1, row: 2 }),
        [Action::Lsp(LspAction::OpenDocument("tests/test.rs".into()))]
    );
    assert_eq!(
        publish_tick(&mut app, Instant::now()),
        vec![Action::Document(DocumentAction::Load(DocumentLoad::Diff {
            review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
            path: "tests/test.rs".to_owned(),
        }))]
    );
    let collapsed = application_screen(&app, 80, 12).join("\n");
    assert!(collapsed.contains("▸ src/"));
    assert!(!collapsed.contains("lib.rs"));
    assert!(!collapsed.contains("main.rs"));

    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "22222222"),
        "Commit title".to_owned(),
        files(),
    );
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("▸ src/")
    );

    app.update(UserInput::MouseClick { column: 1, row: 2 });
    let expanded = application_screen(&app, 80, 12).join("\n");
    assert!(expanded.contains("▾ src/"));
    assert!(expanded.contains("lib.rs"));
    assert!(expanded.contains("main.rs"));
}

#[test]
fn files_that_need_review_expand_their_parent_directories() {
    let mut app = ReviewApplication::default();
    let files = |status| {
        vec![
            FileSummary::new("src/deep/lib.rs", status),
            FileSummary::new("tests/test.rs", ReviewStatus::Reviewed),
        ]
    };
    app.update(UserInput::Resize {
        width: 80,
        height: 12,
    });
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        String::new(),
        files(ReviewStatus::Reviewed),
    );
    app.update(UserInput::MouseClick { column: 1, row: 2 });

    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "22222222"),
        String::new(),
        files(ReviewStatus::ChangedSinceReview),
    );
    let expanded_after_refresh = application_screen(&app, 80, 12).join("\n");
    assert!(
        expanded_after_refresh.contains("▾ src/"),
        "{expanded_after_refresh}"
    );
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("lib.rs")
    );

    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "33333333"),
        String::new(),
        files(ReviewStatus::Reviewed),
    );
    app.update(UserInput::MouseClick { column: 1, row: 2 });
    app.publish(ReviewStateSaved {
        review_unit: "qpvuntsm".into(),
        path: "src/deep/lib.rs".to_owned(),
        result: Ok(ReviewState {
            status: ReviewStatus::Unreviewed,
            warning: None,
            current_diff_statistics: DiffStatistics::default(),
            lines: None,
        }),
    });
    let rendered = application_screen(&app, 80, 12).join("\n");
    assert!(rendered.contains("▾ src/deep/"));
    assert!(rendered.contains("lib.rs"));
}

#[test]
fn dragging_the_separator_resizes_the_file_pane() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    app.update(UserInput::Resize {
        width: 80,
        height: 12,
    });
    let before = screen(&app, 80, 12)[1].find("Diff").unwrap();
    let separator = screen(&app, 80, 12)[1][..before].chars().count() - 2;

    app.update(UserInput::MouseClick {
        column: u16::try_from(separator).unwrap(),
        row: 5,
    });
    app.update(UserInput::MouseDrag { column: 40, row: 5 });
    assert_eq!(
        app.update(UserInput::MouseRelease),
        vec![Action::Settings(SettingsAction::SaveFilePaneWidth(40))]
    );

    let after = screen(&app, 80, 12)[1].find("Diff").unwrap();
    assert!(after > before);

    app.update(UserInput::MouseClick { column: 40, row: 5 });
    app.update(UserInput::MouseDrag { column: 0, row: 5 });
    assert_eq!(
        app.update(UserInput::MouseRelease),
        vec![Action::Settings(SettingsAction::SaveFilePaneWidth(31))]
    );
    assert!(screen(&app, 80, 12)[1].contains("Files   Threads   Explore"));

    app.update(UserInput::MouseClick { column: 31, row: 5 });
    app.update(UserInput::MouseDrag { column: 79, row: 5 });
    assert_eq!(
        app.update(UserInput::MouseRelease),
        vec![Action::Settings(SettingsAction::SaveFilePaneWidth(64))]
    );
}

#[test]
fn dragging_diff_lines_opens_an_inline_comment_on_release() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    app.publish(ui_events::ReviewThreadsLoaded {
        review_unit: "qpvuntsm".into(),
        result: Ok(review_threads::ReviewThreads::new("qpvuntsm".into())),
        drafts: review_threads::SavedDrafts::default(),
    });
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: "src/lib.rs".to_owned(),
        rows: rows(),
        old_content: Some(b"fn run() {\n    old();\n".to_vec()),
        new_content: Some(b"fn run() {\n    new();\n".to_vec()),
        hunks: review_hunks::FileHunks::default(),
    });
    app.update(UserInput::Resize {
        width: 80,
        height: 12,
    });

    app.update(UserInput::MouseClick { column: 70, row: 2 });
    assert!(app.update(UserInput::MouseRelease).is_empty());

    app.update(UserInput::MouseClick { column: 70, row: 2 });
    assert_eq!(app.update(UserInput::MouseDrag { column: 70, row: 4 }), []);
    assert!(app.update(UserInput::MouseRelease).is_empty());
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Vim · INSERT")
    );
    app.update(UserInput::Paste("Please explain this range".into()));
    assert!(app.update(UserInput::Key(Key::Escape)).is_empty());
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Vim · NORMAL")
    );
    assert_eq!(
        app.update(UserInput::Key(Key::EditorMode)),
        [Action::Settings(SettingsAction::SaveEditorKeymap(
            comment_editor::EditorKeymap::Regular
        ))]
    );
    assert!(app.update(UserInput::Key(Key::Escape)).is_empty());
    let screen = application_screen(&app, 80, 12).join("\n");
    assert!(screen.contains("╰─ Regular editing ─"), "{screen}");
    app.set_editor_keymap(comment_editor::EditorKeymap::Vim);
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("╰─ Vim · INSERT ─")
    );
    app.set_editor_keymap(comment_editor::EditorKeymap::Regular);
    app.update(UserInput::Key(Key::Last));
    app.update(UserInput::Key(Key::Char('j')));
    let submit = click_on_text(&application_screen(&app, 80, 12), "Post");
    let saved = app.update(submit);
    assert!(app.update(UserInput::MouseRelease).is_empty());
    let book = acknowledge_comment(&mut app, &saved);
    assert_eq!(book.threads().len(), 1);
    assert_eq!(
        book.threads()[0].messages[0].text,
        "Please explain this rangej"
    );
    assert!(book.threads()[0].excerpt.contains("-    old();"));
    assert!(book.threads()[0].excerpt.contains("+    new();"));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.frame(), frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let color = review_ui::Theme::default().palette.focus;
    for symbol in ["├", "┤", "╰", "╯"] {
        assert!(
            buffer
                .content()
                .iter()
                .any(|cell| cell.symbol() == symbol && cell.fg == color),
            "comment border {symbol} must be visible"
        );
    }
    app.update(UserInput::Key(Key::Tab));
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Files   Threads   Explore")
    );
    assert!(app.update(UserInput::Key(Key::Control('s'))).is_empty());
}

fn acknowledge_comment(
    app: &mut ReviewApplication,
    actions: &[Action],
) -> review_threads::ReviewThreads {
    actions
        .iter()
        .find_map(|action| match action {
            Action::Thread(review_threads::ThreadCommand::Post { review_unit, post }) => {
                let mut book = review_threads::ReviewThreads::new(review_unit.clone());
                book.post(post.clone()).unwrap();
                app.publish(ui_events::ReviewThreadsLoaded {
                    review_unit: review_unit.clone(),
                    result: Ok(book.clone()),
                    drafts: review_threads::SavedDrafts::default(),
                });
                app.publish(ui_events::ThreadPostFinished {
                    review_unit: review_unit.clone(),
                    message_id: post.message().id.clone(),
                    result: Ok(()),
                });
                Some(book)
            }
            _ => None,
        })
        .unwrap()
}

#[test]
fn mouse_wheel_scrolls_the_diff_viewport_regardless_of_focus() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: "src/lib.rs".to_owned(),
        rows: (0..10)
            .map(|index| DiffRow::Context {
                old_line: index + 1,
                new_line: index + 1,
                text: format!(" line-{index}"),
            })
            .collect(),
        old_content: None,
        new_content: None,
        hunks: review_hunks::FileHunks::default(),
    });
    app.update(UserInput::Resize {
        width: 80,
        height: 8,
    });
    let initial = screen(&app, 80, 8).join("\n");
    assert!(initial.contains("line-0"));
    assert!(!initial.contains("line-9"));

    app.update(UserInput::MouseScroll {
        column: 70,
        row: 2,
        delta: 2,
    });
    assert!(!screen(&app, 80, 8).join("\n").contains("line-0"));
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    app.update(UserInput::Resize {
        width: 80,
        height: 8,
    });
    assert!(!screen(&app, 80, 8).join("\n").contains("line-0"));

    app.update(UserInput::MouseClick { column: 70, row: 2 });
    app.update(UserInput::MouseScroll {
        column: 70,
        row: 2,
        delta: 2,
    });
    let screen = screen(&app, 80, 8).join("\n");
    assert!(!screen.contains("line-2"));
    assert!(screen.contains("Diff · src/lib.rs"));
    assert!(pane_has_focus(&app, 80, 8, 70));
}

fn unread_threads(checkpoint: &ReviewCheckpoint, path: &str) -> review_threads::ReviewThreads {
    let mut book = review_threads::ReviewThreads::new(checkpoint.review_unit.clone());
    let post = review_threads::Post::start(
        review_source::DiffRangeAnchor {
            source_checkpoint: checkpoint.checkpoint.clone(),
            old_path: None,
            new_path: Some(path.into()),
            old_lines: None,
            new_lines: None,
            target_kind: review_source::AnchorKind::Hunks,
            source_hunk_count: 0,
            old_content: None,
            new_content: None,
            diff_hash: String::new(),
        },
        String::new(),
        "Check this asset".into(),
    );
    let thread_id = post.thread_id().clone();
    book.post(post).unwrap();
    book.post(review_threads::Post::agent_reply(
        thread_id,
        review_threads::MessageId::parse("7e43f70b-6d54-40ae-b04c-630fa7a6b7ea").unwrap(),
        "The asset is correct".into(),
    ))
    .unwrap();
    book
}

#[test]
fn test_backend_renders_wide_narrow_and_minimum_layouts() {
    let mut app = ReviewApplication::default();
    let mut files = vec![
        FileSummary::new(
            "src/a/very/long/directory/that/must/keep/file.rs",
            ReviewStatus::ChangedSinceReview,
        ),
        FileSummary::new("assets/logo.bin", ReviewStatus::Unreviewed),
    ];
    files.extend(
        (2..200)
            .map(|index| FileSummary::new(format!("src/file-{index}.rs"), ReviewStatus::Reviewed)),
    );
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "Commit title\n\nCommit body\n".to_owned(),
        files,
    );
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: "assets/logo.bin".to_owned(),
        rows: vec![DiffRow::Notice {
            kind: NoticeKind::Binary,
            text: "Binary file; text diff is unavailable".to_owned(),
        }],
        old_content: None,
        new_content: None,
        hunks: review_hunks::FileHunks::default(),
    });
    let mut large_diff = rows();
    large_diff.extend((3..9_995).map(|line| DiffRow::Context {
        old_line: line,
        new_line: line,
        text: format!(" line {line}"),
    }));
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: "src/a/very/long/directory/that/must/keep/file.rs".to_owned(),
        rows: large_diff,
        old_content: None,
        new_content: None,
        hunks: review_hunks::FileHunks::default(),
    });

    let book = unread_threads(
        &ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "assets/logo.bin",
    );
    app.publish(ui_events::ReviewThreadsLoaded {
        review_unit: book.review_unit.clone(),
        result: Ok(book),
        drafts: review_threads::SavedDrafts::default(),
    });

    let wide = application_screen(&app, 120, 30).join("\n");
    assert!(wide.contains("Files   Threads ●   Explore"));
    assert!(wide.contains("Diff · src/a/very/long"));
    assert!(wide.contains("●"));
    assert!(wide.contains('!'));

    let threshold = application_screen(&app, 72, 15).join("\n");
    assert!(threshold.contains("Files   Threads ●"));
    assert!(threshold.contains("Diff ·"));

    app.update(UserInput::Resize {
        width: 60,
        height: 10,
    });
    let narrow_files = application_screen(&app, 60, 10).join("\n");
    assert!(narrow_files.contains("Files   Threads ●   Explore"));
    assert!(!narrow_files.contains("Diff ·"));
    assert!(narrow_files.contains("file.rs"));

    app.update(UserInput::Key(Key::Tab));
    let narrow_diff = application_screen(&app, 60, 10).join("\n");
    assert!(narrow_diff.contains("Diff ·"));
    assert!(!narrow_diff.contains("Files   Threads"));

    let minimum = application_screen(&app, 40, 6).join("\n");
    assert!(minimum.starts_with(" abcd1234  Com"), "{minimum}");
    assert!(minimum.contains("99% reviewed"));
    let too_small = application_screen(&app, 39, 5);
    assert_eq!(too_small[0].trim_end(), "Terminal is too small");
    assert_eq!(too_small[1].trim_end(), "Minimum: 40x6");
    assert_eq!(too_small[2].trim_end(), "q quit");

    app.update(UserInput::Key(Key::Tab));
    app.update(UserInput::Key(Key::Down));
    app.update(UserInput::Key(Key::Tab));
    app.update(UserInput::Key(Key::Char('V')));
    assert!(
        application_screen(&app, 80, 10)
            .last()
            .unwrap()
            .contains("? help")
    );
}

#[test]
fn question_mark_opens_shortcut_help_and_escape_closes_it() {
    let mut app = wrapped_diff_application(rows(), 100, 30);

    assert!(app.update(UserInput::Key(Key::Char('?'))).is_empty());
    let popup = screen(&app, 100, 30).join("\n");
    assert!(popup.contains("Keyboard shortcuts"));
    assert!(popup.contains("[h / ]h"));
    assert!(popup.contains("[f / ]f"));
    assert!(!popup.contains("Review guide"));

    assert!(app.update(UserInput::Key(Key::Escape)).is_empty());
    assert!(
        !screen(&app, 100, 30)
            .join("\n")
            .contains("Keyboard shortcuts")
    );
}

#[test]
fn shortcut_help_scrolls_on_short_terminals() {
    let mut app = wrapped_diff_application(rows(), 80, 6);

    app.update(UserInput::Key(Key::Char('?')));
    assert!(!screen(&app, 80, 6).join("\n").contains("Quit"));
    let mut popup = screen(&app, 80, 6).join("\n");
    loop {
        app.update(UserInput::Key(Key::Down));
        let scrolled = screen(&app, 80, 6).join("\n");
        if scrolled == popup {
            break;
        }
        popup = scrolled;
    }

    // The last lines of the help.
    assert!(popup.contains("Explore page settings: turn over opening the page"));
    assert!(popup.contains("Explore page settings: edit the network interface"));
}

#[test]
fn the_open_tab_is_a_filled_pill_with_its_key_underlined() {
    let mut app = wrapped_diff_application(rows(), 80, 12);
    let palette = review_ui::Theme::default().palette;
    let style_at = |app: &ReviewApplication, column: u16| {
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(app.frame(), frame.area()))
            .unwrap();
        terminal.backend().buffer()[(column, 1)].clone()
    };
    // The tabs start after the pane's corner: " Files " then " Threads ".
    let (files_key, threads_key) = (2, 10);

    let files = style_at(&app, files_key);
    assert_eq!(files.symbol(), "F");
    assert_eq!(files.bg, palette.focus);
    assert!(files.modifier.contains(Modifier::UNDERLINED));
    let threads = style_at(&app, threads_key);
    assert_eq!(threads.symbol(), "T");
    assert_eq!(threads.fg, palette.dim);
    assert!(threads.modifier.contains(Modifier::UNDERLINED));

    app.update(UserInput::Key(Key::Char('t')));
    assert_eq!(style_at(&app, threads_key).bg, palette.focus);
    assert_ne!(style_at(&app, files_key).bg, palette.focus);
}
