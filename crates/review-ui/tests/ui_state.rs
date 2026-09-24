use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use review_guide::{GuideItem, GuideItemStatus, GuideLineRange, GuideTarget, ReviewCheckpoint};
use review_repository::{
    diff::{DiffRow, NoticeKind},
    repository::DiffStatistics,
};
use review_state::{ReviewState, ReviewStatus};
use review_ui::{Action, Key, ReviewApplication, UserInput};
use std::time::Instant;
use ui_events::{
    AnimationTick, FileSummary, RepositoryFilesChanged, RepositoryMetadataChanged,
    ReviewGuideChanged, ReviewGuideStatusChanged, ReviewStateSaved, ToastExpirationTick,
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
    });
    application.update(UserInput::Resize { width, height });
    application.update(UserInput::Key(Key::Tab));
    application
}

fn load_single_hunk_guide_in_application(application: &mut ReviewApplication, text: &str) {
    application.publish(ReviewGuideChanged {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        items: vec![GuideItem {
            target: GuideTarget::Hunks {
                path: "src/lib.rs".to_owned(),
                first_hunk: 1,
                last_hunk: 1,
            },
            text: text.to_owned(),
            status: GuideItemStatus::Matched,
        }],
    });
}

#[test]
fn review_guide_wraps_only_changed_hunk_rows_with_the_explanation_at_the_top() {
    let mut guide_rows = rows();
    let DiffRow::Add { text, .. } = guide_rows.last_mut().unwrap() else {
        unreachable!();
    };
    *text = format!("+{}", " wrapped-source".repeat(8));
    let mut app = wrapped_diff_application(guide_rows, 42, 20);
    load_single_hunk_guide_in_application(
        &mut app,
        "This central explanation wraps at spaces and stays visible.",
    );

    let rendered = application_screen(&app, 42, 20);
    let joined = rendered.join("\n");
    assert!(joined.contains("wrapped-source"));
    assert!(joined.contains("╭─"));
    assert!(joined.contains("├─"));
    assert!(joined.contains("╰─"));
    assert!(joined.contains("╮│"), "{joined}");
    assert!(joined.contains("┤│"), "{joined}");
    assert!(joined.contains("╯│"), "{joined}");
    assert!(!joined.contains("▌│"), "{joined}");
    assert!(joined.contains("│▌ 2│ wrapped-source"), "{joined}");
    assert!(
        joined.contains("│   │  This central explanation"),
        "{joined}"
    );
    assert!(joined.contains("This central explanation"));
    assert!(joined.contains("wraps"));
    assert!(joined.contains("spaces and stays visible"));
    assert!(
        joined.find("fn run()").unwrap() < joined.find("This central explanation").unwrap(),
        "{joined}"
    );
    assert!(
        rendered
            .iter()
            .filter(|line| line.contains("wrapped-source"))
            .all(|line| line.ends_with("││")),
        "{joined}"
    );
    assert!(rendered.iter().all(|line| line.chars().count() == 42));
}

#[test]
fn guide_border_overlay_preserves_changed_row_backgrounds() {
    let mut app = wrapped_diff_application(rows(), 60, 20);
    load_single_hunk_guide_in_application(&mut app, "A short explanation.");
    let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.frame(), frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();

    let colored_border_cells = buffer
        .content()
        .iter()
        .filter(|cell| {
            cell.symbol() == "│" && cell.fg == Color::LightYellow && cell.bg != Color::Reset
        })
        .collect::<Vec<_>>();
    assert!(
        colored_border_cells.len() >= 2,
        "colored border cells: {colored_border_cells:?}"
    );
}

#[test]
fn file_with_a_review_guide_has_a_comment_marker() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    );
    assert!(!application_screen(&app, 100, 12).join("\n").contains(" 📄"));

    app.publish(ReviewGuideChanged {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        items: vec![GuideItem {
            target: GuideTarget::Hunks {
                path: "src/lib.rs".to_owned(),
                first_hunk: 1,
                last_hunk: 1,
            },
            text: "A short explanation.".to_owned(),
            status: GuideItemStatus::Matched,
        }],
    });

    assert!(
        application_screen(&app, 100, 12)
            .iter()
            .any(|line| line.contains("lib.rs") && line.contains(" 📄"))
    );
}

#[test]
fn guide_generation_spinner_is_at_the_right_of_the_status_line() {
    let mut app = wrapped_diff_application(rows(), 80, 12);
    app.publish(ReviewGuideStatusChanged {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        generating: true,
        message: None,
    });
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.frame(), frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let first = (0..80)
        .map(|column| buffer[(column, 11)].symbol())
        .collect::<String>();
    assert!(first.starts_with("? help"), "{first}");
    assert!(first.ends_with("⠋ Generating guide"), "{first}");
    assert_ne!(buffer[(0, 11)].fg, Color::LightYellow);
    assert_eq!(buffer[(79, 11)].fg, Color::LightYellow);

    publish_tick(&mut app, Instant::now());
    let second = application_screen(&app, 80, 12)[11].clone();
    assert!(second.ends_with("⠙ Generating guide"), "{second}");
}

#[test]
fn separate_line_guides_render_inside_one_hunk() {
    let mut guide_rows = vec![DiffRow::Hunk {
        old_start: 0,
        old_count: 0,
        new_start: 1,
        new_count: 6,
    }];
    guide_rows.extend((1..=6).map(|line| DiffRow::Add {
        new_line: line,
        text: format!("+line_{line}"),
    }));
    let mut app = wrapped_diff_application(guide_rows, 60, 24);
    app.publish(ReviewGuideChanged {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        items: [
            (5, 6, "Second idea."),
            (50, 50, "Unmapped idea."),
            (1, 2, "First idea."),
        ]
        .into_iter()
        .map(|(first_line, last_line, text)| GuideItem {
            target: GuideTarget::Lines {
                path: "src/lib.rs".to_owned(),
                old: None,
                new: Some(GuideLineRange {
                    first_line,
                    last_line,
                }),
            },
            text: text.to_owned(),
            status: GuideItemStatus::Matched,
        })
        .collect(),
    });

    let rendered = application_screen(&app, 60, 24).join("\n");

    assert!(rendered.contains("First idea."), "{rendered}");
    assert!(rendered.contains("Second idea."), "{rendered}");
    assert!(!rendered.contains("Unmapped idea."), "{rendered}");
    assert!(rendered.contains("line_3"), "{rendered}");
    assert!(rendered.contains("line_4"), "{rendered}");
    assert_eq!(rendered.matches('╭').count(), 2, "{rendered}");
    assert_eq!(rendered.matches('╰').count(), 2, "{rendered}");
    assert!(rendered.contains(" 1/2 ╮"), "{rendered}");
    assert!(rendered.contains(" 2/2 ╮"), "{rendered}");
    assert!(!rendered.contains("/3"), "{rendered}");
    let first_counter = rendered.find(" 1/2 ╮").unwrap();
    let first_text = rendered.find("First idea.").unwrap();
    let second_counter = rendered.find(" 2/2 ╮").unwrap();
    let second_text = rendered.find("Second idea.").unwrap();
    assert!(first_counter < first_text, "{rendered}");
    assert!(first_text < second_counter, "{rendered}");
    assert!(second_counter < second_text, "{rendered}");
}

#[test]
fn guide_counter_includes_items_for_diffs_that_are_not_loaded() {
    let mut app = ReviewApplication::default();
    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        String::new(),
        vec![
            FileSummary::new("src/first.rs", ReviewStatus::Unreviewed),
            FileSummary::new("src/second.rs", ReviewStatus::Unreviewed),
        ],
    );
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: "src/first.rs".to_owned(),
        rows: rows(),
        old_content: None,
        new_content: None,
    });
    app.publish(ReviewGuideChanged {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        items: ["src/first.rs", "src/second.rs"]
            .into_iter()
            .map(|path| GuideItem {
                target: GuideTarget::Hunks {
                    path: path.to_owned(),
                    first_hunk: 1,
                    last_hunk: 1,
                },
                text: format!("Guide for {path}"),
                status: GuideItemStatus::Matched,
            })
            .collect(),
    });
    app.update(UserInput::Resize {
        width: 60,
        height: 12,
    });
    app.update(UserInput::Key(Key::Tab));

    let rendered = application_screen(&app, 60, 12).join("\n");

    assert!(rendered.contains(" 1/2 ╮"), "{rendered}");
}

#[test]
fn gg_keeps_the_top_of_a_guided_hunk_at_the_top() {
    let mut guide_rows = vec![DiffRow::Hunk {
        old_start: 0,
        old_count: 0,
        new_start: 1,
        new_count: 30,
    }];
    guide_rows.extend((1..=30).map(|line| DiffRow::Add {
        new_line: line,
        text: format!("+line_{line}"),
    }));
    let mut app = wrapped_diff_application(guide_rows, 60, 12);
    load_single_hunk_guide_in_application(&mut app, "Top explanation.");

    app.update(UserInput::Key(Key::Char('G')));
    app.update(UserInput::Key(Key::Char('g')));
    app.update(UserInput::Key(Key::Char('g')));

    let rendered = application_screen(&app, 60, 12).join("\n");
    assert!(rendered.contains("Top explanation."), "{rendered}");
    assert!(rendered.contains("line_1"), "{rendered}");
    assert!(!rendered.contains("line_30"), "{rendered}");
}

#[test]
fn guide_comment_jumps_align_the_top_border_with_the_viewport_top() {
    let mut guide_rows = vec![DiffRow::Hunk {
        old_start: 1,
        old_count: 30,
        new_start: 1,
        new_count: 30,
    }];
    guide_rows.extend((1..=30).map(|line| DiffRow::Context {
        old_line: line,
        new_line: line,
        text: format!(" line_{line}"),
    }));
    let mut app = wrapped_diff_application(guide_rows, 60, 12);
    app.publish(ReviewGuideChanged {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        items: vec![GuideItem {
            target: GuideTarget::Lines {
                path: "src/lib.rs".to_owned(),
                old: None,
                new: Some(GuideLineRange {
                    first_line: 25,
                    last_line: 25,
                }),
            },
            text: "Late explanation.".to_owned(),
            status: GuideItemStatus::Matched,
        }],
    });

    app.update(UserInput::Key(Key::Char(']')));
    app.update(UserInput::Key(Key::Char('r')));
    let next = application_screen(&app, 60, 12);
    assert!(next[2].contains('╭'), "{}", next.join("\n"));
    assert!(next[3].contains("Late explanation."), "{}", next.join("\n"));
    assert!(next[5].contains("line_25"), "{}", next.join("\n"));

    app.update(UserInput::Key(Key::Char('G')));
    app.update(UserInput::Key(Key::Char('[')));
    app.update(UserInput::Key(Key::Char('r')));
    let previous = application_screen(&app, 60, 12);
    assert!(previous[2].contains('╭'), "{}", previous.join("\n"));
    assert!(
        previous[3].contains("Late explanation."),
        "{}",
        previous.join("\n")
    );
    assert!(previous[5].contains("line_25"), "{}", previous.join("\n"));
}

#[test]
fn reviewed_file_hides_its_review_guide() {
    let mut app = wrapped_diff_application(rows(), 60, 12);
    load_single_hunk_guide_in_application(&mut app, "This guide is hidden.");
    assert!(
        application_screen(&app, 60, 12)
            .join("\n")
            .contains("This guide")
    );

    publish_repository(
        &mut app,
        ReviewCheckpoint::new("qpvuntsm", "11111111"),
        String::new(),
        vec![FileSummary::new("src/lib.rs", ReviewStatus::Reviewed)],
    );

    assert!(
        !application_screen(&app, 60, 12)
            .join("\n")
            .contains("This guide")
    );
}

#[test]
fn stale_review_guide_explanations_are_dimmed() {
    let mut app = wrapped_diff_application(rows(), 60, 12);
    app.publish(ReviewGuideChanged {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        items: vec![GuideItem {
            target: GuideTarget::Hunks {
                path: "src/lib.rs".to_owned(),
                first_hunk: 1,
                last_hunk: 1,
            },
            text: "ZZZ carried explanation".to_owned(),
            status: GuideItemStatus::Stale,
        }],
    });
    let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.frame(), frame.area()))
        .unwrap();

    let stale_text_cells = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .filter(|cell| cell.symbol() == "Z")
        .collect::<Vec<_>>();
    assert!(!stale_text_cells.is_empty());
    assert!(
        stale_text_cells
            .iter()
            .all(|cell| cell.modifier.contains(Modifier::DIM))
    );
    assert!(stale_text_cells.iter().all(|cell| cell.bg == Color::Reset));
    assert!(
        stale_text_cells
            .iter()
            .all(|cell| cell.fg == Color::LightYellow)
    );
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
    let [Action::Lsp { operation, query }] = actions.as_slice() else {
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
    let [Action::Lsp { query, .. }] = actions.as_slice() else {
        panic!("a single type definition must jump directly to the source");
    };
    assert_eq!((query.line, query.byte_column), (0, 7));
    assert_eq!(query.expected_line, "struct Thing;");
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
    let [Action::Lsp { query, .. }] = actions.as_slice() else {
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
    let [Action::Lsp { query, .. }] = actions.as_slice() else {
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
            Action::LoadDiff {
                review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
                path: "src/lib.rs".to_owned(),
            },
            Action::OpenLspDocument("src/lib.rs".into())
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
    });

    app.update(UserInput::Key(Key::Tab));
    app.update(UserInput::Key(Key::Visual));
    app.update(UserInput::Key(Key::Down));
    app.update(UserInput::Key(Key::Visual));
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
            .contains("[F]iles | [T]hreads")
    );
    application.update(UserInput::Key(Key::Char('/')));
    assert!(
        screen(&application, 80, 12)
            .join("\n")
            .contains("Diff · src/lib.rs (focus)")
    );
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
    assert!(header.contains("+0 -0 - 0/0 reviewed"));
    assert!(!header.contains("Progressive review"));
    assert!(!header.contains("change qpvuntsm"));

    app.update(UserInput::Key(Key::CommitMessage));
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
        app.update(UserInput::Key(Key::Space)).as_slice(),
        [
            Action::SetReviewed { reviewed: true, .. },
            Action::OpenLspDocument(_)
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
        vec![Action::LoadDiff {
            review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
            path: "third.rs".to_owned(),
        }]
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
        .find(|column| buffer[(*column, 1)].symbol() == "┐")
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
    });
    app.update(UserInput::Resize {
        width: 100,
        height: 14,
    });

    let collapsed = screen(&app, 100, 14).join("\n");
    assert!(collapsed.contains("←→"));
    assert!(collapsed.contains("→←"));
    assert!(collapsed.contains('👁'));
    assert!(collapsed.contains("1 unmodified lines"));
    app.update(UserInput::MouseClick { column: 87, row: 1 });
    assert!(
        !screen(&app, 100, 14)
            .join("\n")
            .contains("unmodified lines")
    );
    app.update(UserInput::MouseClick { column: 92, row: 1 });
    assert!(
        screen(&app, 100, 14)
            .join("\n")
            .contains("1 unmodified lines")
    );

    app.update(UserInput::MouseClick { column: 96, row: 1 });
    let file = screen(&app, 100, 14).join("\n");
    assert!(file.contains("File ·"));
    assert!(file.contains("[x]"));
    assert!(!file.contains("←→"));
    assert!(!file.contains("→←"));
    assert!(file.contains("2 middle"));
    assert!(!file.contains("unmodified lines"));

    app.update(UserInput::MouseClick { column: 97, row: 1 });
    let diff = screen(&app, 100, 14).join("\n");
    assert!(diff.contains("Diff ·"));
    assert!(diff.contains("←→"));
    assert!(diff.contains("→←"));
    assert!(diff.contains("1 unmodified lines"));
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
    });

    assert_eq!(
        app.update(UserInput::Key(Key::Space)),
        vec![Action::SetReviewed {
            path: "src/lib.rs".to_owned(),
            reviewed: true,
        }]
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
        app.update(UserInput::Key(Key::Space)).as_slice(),
        [Action::SetReviewed { reviewed: true, .. }]
    ));
    assert_eq!(
        app.publish(ReviewStateSaved {
            review_unit: "qpvuntsm".into(),
            path: "src/lib.rs".to_owned(),
            result: Ok(ReviewState {
                status: ReviewStatus::Reviewed,
                warning: None,
                current_diff_statistics: DiffStatistics::default(),
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
        [Action::OpenLspDocument("second.rs".into())]
    );
    assert_eq!(
        publish_tick(&mut app, Instant::now()),
        vec![Action::LoadDiff {
            review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
            path: "second.rs".to_owned(),
        }]
    );
    app.update(UserInput::MouseClick { column: 70, row: 2 });
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Diff · second.rs (focus)")
    );
    app.update(UserInput::MouseScroll {
        column: 1,
        row: 2,
        delta: -1,
    });
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Diff · first.rs (focus)")
    );
    app.update(UserInput::MouseClick { column: 1, row: 3 });
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Diff · second.rs")
    );
    assert!(app.update(UserInput::Key(Key::Enter)).is_empty());
    assert_eq!(
        app.update(UserInput::MouseControlClick { column: 1, row: 2 }),
        vec![Action::OpenLspDocument("first.rs".into())]
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
            Action::SetReviewed {
                path: "first.rs".to_owned(),
                reviewed: true,
            },
            Action::OpenLspDocument("second.rs".into())
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
            Action::SetReviewed {
                path: "reviewed.rs".to_owned(),
                reviewed: false,
            },
            Action::LoadDiff {
                review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
                path: "reviewed.rs".to_owned(),
            },
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
        [Action::OpenLspDocument("tests/test.rs".into())]
    );
    assert_eq!(
        publish_tick(&mut app, Instant::now()),
        vec![Action::LoadDiff {
            review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
            path: "tests/test.rs".to_owned(),
        }]
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
        vec![Action::SaveFilePaneWidth(40)]
    );

    let after = screen(&app, 80, 12)[1].find("Diff").unwrap();
    assert!(after > before);

    app.update(UserInput::MouseClick { column: 40, row: 5 });
    app.update(UserInput::MouseDrag { column: 0, row: 5 });
    assert_eq!(
        app.update(UserInput::MouseRelease),
        vec![Action::SaveFilePaneWidth(37)]
    );
    assert!(screen(&app, 80, 12)[1].contains("[F]iles | [T]hreads"));

    app.update(UserInput::MouseClick { column: 37, row: 5 });
    app.update(UserInput::MouseDrag { column: 79, row: 5 });
    assert_eq!(
        app.update(UserInput::MouseRelease),
        vec![Action::SaveFilePaneWidth(64)]
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
    });
    app.publish(ui_events::DiffContentLoaded {
        review_checkpoint: ReviewCheckpoint::new("qpvuntsm", "11111111"),
        path: "src/lib.rs".to_owned(),
        rows: rows(),
        old_content: Some(b"fn run() {\n    old();\n".to_vec()),
        new_content: Some(b"fn run() {\n    new();\n".to_vec()),
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
    assert!(app.update(UserInput::Key(Key::EditorMode)).is_empty());
    assert!(app.update(UserInput::Key(Key::Escape)).is_empty());
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("Regular editing")
    );
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
            "comment border {symbol} must be visible without a review guide"
        );
    }
    app.update(UserInput::Key(Key::Tab));
    assert!(
        application_screen(&app, 80, 12)
            .join("\n")
            .contains("[F]iles | [T]hreads")
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
    assert!(screen.contains("Diff · src/lib.rs (focus)"));
}

fn unread_threads(checkpoint: &ReviewCheckpoint, path: &str) -> review_threads::ReviewThreads {
    let mut book = review_threads::ReviewThreads::new(checkpoint.review_unit.clone());
    let post = review_threads::Post::start(
        review_guide::DiffRangeAnchor {
            source_checkpoint: checkpoint.checkpoint.clone(),
            old_path: None,
            new_path: Some(path.into()),
            old_lines: None,
            new_lines: None,
            target_kind: review_guide::GuideAnchorKind::Hunks,
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
    });

    let book = unread_threads(
        &ReviewCheckpoint::new("qpvuntsm", "11111111"),
        "assets/logo.bin",
    );
    app.publish(ui_events::ReviewThreadsLoaded {
        review_unit: book.review_unit.clone(),
        result: Ok(book),
    });

    let wide = application_screen(&app, 120, 30).join("\n");
    assert!(wide.contains("[F]iles | [T]hreads"));
    assert!(wide.contains("Diff · src/a/very/long"));
    assert!(wide.contains("●"));
    assert!(wide.contains('!'));

    let threshold = application_screen(&app, 72, 15).join("\n");
    assert!(threshold.contains("[F]iles | [T]hreads ●"));
    assert!(threshold.contains("Diff ·"));

    app.update(UserInput::Resize {
        width: 60,
        height: 10,
    });
    let narrow_files = application_screen(&app, 60, 10).join("\n");
    assert!(narrow_files.contains("[F]iles | [T]hreads"));
    assert!(!narrow_files.contains("Diff ·"));
    assert!(narrow_files.contains("file.rs"));

    app.update(UserInput::Key(Key::Tab));
    let narrow_diff = application_screen(&app, 60, 10).join("\n");
    assert!(narrow_diff.contains("Diff ·"));
    assert!(!narrow_diff.contains("[F]iles | [T]hreads"));

    let minimum = application_screen(&app, 40, 6).join("\n");
    assert!(minimum.starts_with(" abcd1234 Comm"), "{minimum}");
    assert!(minimum.contains("198/200 reviewed"));
    let too_small = application_screen(&app, 39, 5);
    assert_eq!(too_small[0].trim_end(), "Terminal is too small");
    assert_eq!(too_small[1].trim_end(), "Minimum: 40x6");
    assert_eq!(too_small[2].trim_end(), "q quit");

    app.update(UserInput::Key(Key::Tab));
    app.update(UserInput::Key(Key::Down));
    app.update(UserInput::Key(Key::Tab));
    app.update(UserInput::Key(Key::Visual));
    assert!(
        application_screen(&app, 80, 10)
            .last()
            .unwrap()
            .contains("? help")
    );
}

#[test]
fn question_mark_opens_shortcut_help_and_escape_closes_it() {
    let mut app = wrapped_diff_application(rows(), 80, 24);

    assert!(app.update(UserInput::Key(Key::Char('?'))).is_empty());
    let popup = screen(&app, 80, 24).join("\n");
    assert!(popup.contains("Keyboard shortcuts"));
    assert!(popup.contains("rf / ra"));
    assert!(popup.contains("[r / ]r"));

    assert!(app.update(UserInput::Key(Key::Escape)).is_empty());
    assert!(
        !screen(&app, 80, 24)
            .join("\n")
            .contains("Keyboard shortcuts")
    );
}

#[test]
fn shortcut_help_scrolls_on_short_terminals() {
    let mut app = wrapped_diff_application(rows(), 80, 6);

    app.update(UserInput::Key(Key::Char('?')));
    assert!(!screen(&app, 80, 6).join("\n").contains("Quit"));
    for _ in 0..ui_shortcuts::help_line_count() {
        app.update(UserInput::Key(Key::Down));
    }

    let popup = screen(&app, 80, 6).join("\n");
    assert!(popup.contains("Visit previous / next interview turn"));
    assert!(popup.contains("Fit Explore evidence"));
}
