use pr_app::review::{ReviewState, ReviewStatus, ReviewWarning};
use pr_app::ui::{Action, Key, Message, ReviewApp, ReviewFile};
use pr_core::diff::{DiffRow, NoticeKind};
use pr_core::herdr::InsertResult;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;

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

fn screen(app: &ReviewApp, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app.view(), frame.area()))
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

#[test]
fn state_machine_keeps_selection_until_insert_succeeds() {
    let mut app = ReviewApp::default();
    assert_eq!(
        app.update(Message::FilesLoaded {
            change_id: "qpvuntsm".to_owned(),
            commit_id: "11111111".to_owned(),
            files: vec![
                ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed),
                ReviewFile::new("README.md", ReviewStatus::Reviewed),
            ],
        }),
        Action::LoadDiff {
            commit_id: "11111111".to_owned(),
            path: "src/lib.rs".to_owned(),
        }
    );
    app.update(Message::DiffLoaded {
        commit_id: "stale".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: vec![DiffRow::Meta {
            text: "stale result".to_owned(),
        }],
        old_content: None,
        new_content: None,
    });
    assert!(!screen(&app, 80, 12).join("\n").contains("stale result"));
    app.update(Message::DiffLoaded {
        commit_id: "11111111".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: rows(),
        old_content: None,
        new_content: None,
    });

    app.update(Message::Key(Key::Tab));
    app.update(Message::Key(Key::Visual));
    app.update(Message::Key(Key::Down));
    app.update(Message::Key(Key::Visual));
    assert_eq!(
        app.update(Message::Key(Key::Enter)),
        Action::Insert {
            text: concat!(
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -1,2 +1,1 @@\n",
                " fn run() {\n",
                "-    old();"
            )
            .to_owned(),
        }
    );

    app.update(Message::InsertFinished(Err("agent unavailable".to_owned())));
    assert!(matches!(
        app.update(Message::Key(Key::Enter)),
        Action::Insert { .. }
    ));
    app.update(Message::InsertFinished(Ok(InsertResult::NoAgent)));
    assert!(
        screen(&app, 80, 12)
            .join("\n")
            .contains("No agent chat is available in this workspace")
    );
    assert!(matches!(
        app.update(Message::Key(Key::Enter)),
        Action::Insert { .. }
    ));
    app.update(Message::InsertFinished(Ok(InsertResult::Inserted {
        agent_name: "Codex".to_owned(),
    })));
    assert!(
        screen(&app, 80, 12)
            .join("\n")
            .contains("Inserted into Codex")
    );
    assert_eq!(app.update(Message::Key(Key::Enter)), Action::None);

    assert_eq!(
        app.update(Message::Key(Key::Space)),
        Action::SetReviewed {
            path: "src/lib.rs".to_owned(),
            reviewed: true,
        }
    );
    assert_eq!(app.update(Message::Key(Key::Space)), Action::None);
    app.update(Message::ReviewFinished {
        change_id: "qpvuntsm".to_owned(),
        path: "src/lib.rs".to_owned(),
        result: Ok(ReviewState {
            status: ReviewStatus::Unreviewed,
            warning: Some(ReviewWarning::BaselineExpired),
        }),
    });
    assert!(
        screen(&app, 80, 12)
            .join("\n")
            .contains("Review baseline expired; file reset to unreviewed")
    );
}

#[test]
fn reviewed_file_hides_its_diff() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "qpvuntsm".to_owned(),
        commit_id: "11111111".to_owned(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Reviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "11111111".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: rows(),
        old_content: None,
        new_content: None,
    });

    let screen = screen(&app, 80, 12);
    assert!(screen[5].contains("No changes"));
    assert!(!screen.join("\n").contains("old();"));
}

#[test]
fn added_file_renders_as_plain_file_content() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "qpvuntsm".to_owned(),
        commit_id: "11111111".to_owned(),
        files: vec![ReviewFile::new("src/main.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "11111111".to_owned(),
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
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "qpvuntsm".to_owned(),
        commit_id: "11111111".to_owned(),
        files: vec![ReviewFile::new("src/main.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "11111111".to_owned(),
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
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "qpvuntsm".to_owned(),
        commit_id: "11111111".to_owned(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
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
    let load = |app: &mut ReviewApp| {
        app.update(Message::DiffLoaded {
            commit_id: "11111111".to_owned(),
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
        .draw(|frame| frame.render_widget(app.view(), frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_ne!(buffer[(31, 5)].bg, Color::Reset);
    assert_eq!(buffer[(31, 5)].bg, buffer[(98, 5)].bg);

    app.update(Message::Key(Key::Tab));
    for _ in 0..3 {
        app.update(Message::Key(Key::Down));
    }
    app.update(Message::Key(Key::Expand));
    let expanded = screen(&app, 100, 14).join("\n");
    assert!(expanded.contains("3 third"));
    assert!(!expanded.contains("… 3 unmodified lines"));

    load(&mut app);
    app.update(Message::MouseClick {
        column: 70,
        row: 5,
        insert_path: false,
    });
    assert!(screen(&app, 100, 14).join("\n").contains("3 third"));
}

#[test]
fn removing_a_changed_review_mark_loads_the_full_diff() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "qpvuntsm".to_owned(),
        commit_id: "11111111".to_owned(),
        files: vec![ReviewFile::new(
            "src/lib.rs",
            ReviewStatus::ChangedSinceReview,
        )],
    });
    app.update(Message::DiffLoaded {
        commit_id: "11111111".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: rows(),
        old_content: None,
        new_content: None,
    });

    assert_eq!(
        app.update(Message::Key(Key::Space)),
        Action::SetReviewed {
            path: "src/lib.rs".to_owned(),
            reviewed: false,
        }
    );
    assert_eq!(
        app.update(Message::ReviewFinished {
            change_id: "qpvuntsm".to_owned(),
            path: "src/lib.rs".to_owned(),
            result: Ok(ReviewState {
                status: ReviewStatus::Unreviewed,
                warning: None,
            }),
        }),
        Action::LoadDiff {
            commit_id: "11111111".to_owned(),
            path: "src/lib.rs".to_owned(),
        }
    );
}

#[test]
fn mouse_targets_the_hovered_pane_and_click_changes_focus() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "qpvuntsm".to_owned(),
        commit_id: "11111111".to_owned(),
        files: vec![
            ReviewFile::new("first.rs", ReviewStatus::Unreviewed),
            ReviewFile::new("second.rs", ReviewStatus::Unreviewed),
        ],
    });
    app.update(Message::Resize {
        width: 80,
        height: 12,
    });

    assert_eq!(
        app.update(Message::MouseScroll {
            column: 1,
            row: 2,
            delta: 1,
        }),
        Action::LoadDiff {
            commit_id: "11111111".to_owned(),
            path: "second.rs".to_owned(),
        }
    );
    app.update(Message::MouseClick {
        column: 70,
        row: 2,
        insert_path: false,
    });
    assert!(
        screen(&app, 80, 12)
            .join("\n")
            .contains("Diff · second.rs (focus)")
    );
    app.update(Message::MouseScroll {
        column: 1,
        row: 2,
        delta: -1,
    });
    assert!(
        screen(&app, 80, 12)
            .join("\n")
            .contains("Diff · first.rs (focus)")
    );
    app.update(Message::MouseClick {
        column: 1,
        row: 3,
        insert_path: false,
    });
    assert!(screen(&app, 80, 12).join("\n").contains("Diff · second.rs"));
    assert_eq!(
        app.update(Message::Key(Key::Enter)),
        Action::Insert {
            text: "second.rs".to_owned(),
        }
    );
    assert_eq!(
        app.update(Message::MouseClick {
            column: 1,
            row: 2,
            insert_path: true,
        }),
        Action::Insert {
            text: "first.rs".to_owned(),
        }
    );
}

#[test]
fn dragging_the_separator_resizes_the_file_pane() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "qpvuntsm".to_owned(),
        commit_id: "11111111".to_owned(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::Resize {
        width: 80,
        height: 12,
    });
    let before = screen(&app, 80, 12)[1].find("Diff").unwrap();

    app.update(Message::MouseClick {
        column: 24,
        row: 5,
        insert_path: false,
    });
    app.update(Message::MouseDrag { column: 40, row: 5 });
    app.update(Message::MouseRelease);

    let after = screen(&app, 80, 12)[1].find("Diff").unwrap();
    assert!(after > before);
}

#[test]
fn dragging_diff_lines_inserts_them_on_release() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "qpvuntsm".to_owned(),
        commit_id: "11111111".to_owned(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "11111111".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: rows(),
        old_content: None,
        new_content: None,
    });
    app.update(Message::Resize {
        width: 80,
        height: 12,
    });

    app.update(Message::MouseClick {
        column: 70,
        row: 2,
        insert_path: false,
    });
    assert_eq!(app.update(Message::MouseRelease), Action::None);

    app.update(Message::MouseClick {
        column: 70,
        row: 2,
        insert_path: false,
    });
    assert_eq!(
        app.update(Message::MouseDrag { column: 70, row: 4 }),
        Action::None
    );
    assert_eq!(
        app.update(Message::MouseRelease),
        Action::Insert {
            text: concat!(
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -1,2 +1,2 @@\n",
                " fn run() {\n",
                "-    old();\n",
                "+    new();"
            )
            .to_owned(),
        }
    );
}

#[test]
fn mouse_wheel_scrolls_the_diff_viewport_regardless_of_focus() {
    let mut app = ReviewApp::default();
    app.update(Message::FilesLoaded {
        change_id: "qpvuntsm".to_owned(),
        commit_id: "11111111".to_owned(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "11111111".to_owned(),
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
    app.update(Message::Resize {
        width: 80,
        height: 8,
    });
    let initial = screen(&app, 80, 8).join("\n");
    assert!(initial.contains("line-0"));
    assert!(!initial.contains("line-9"));

    app.update(Message::MouseScroll {
        column: 70,
        row: 2,
        delta: 2,
    });
    assert!(!screen(&app, 80, 8).join("\n").contains("line-0"));
    app.update(Message::FilesLoaded {
        change_id: "qpvuntsm".to_owned(),
        commit_id: "11111111".to_owned(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::Resize {
        width: 80,
        height: 8,
    });
    assert!(!screen(&app, 80, 8).join("\n").contains("line-0"));

    app.update(Message::MouseClick {
        column: 70,
        row: 2,
        insert_path: false,
    });
    app.update(Message::MouseScroll {
        column: 70,
        row: 2,
        delta: 2,
    });
    let screen = screen(&app, 80, 8).join("\n");
    assert!(!screen.contains("line-2"));
    assert!(screen.contains("Diff · src/lib.rs (focus)"));
}

#[test]
fn test_backend_renders_wide_narrow_and_minimum_layouts() {
    let mut app = ReviewApp::default();
    let mut files = vec![
        ReviewFile::new(
            "src/a/very/long/directory/that/must/keep/file.rs",
            ReviewStatus::ChangedSinceReview,
        ),
        ReviewFile::new("assets/logo.bin", ReviewStatus::Unreviewed),
    ];
    files.extend(
        (2..200)
            .map(|index| ReviewFile::new(format!("src/file-{index}.rs"), ReviewStatus::Reviewed)),
    );
    app.update(Message::FilesLoaded {
        change_id: "qpvuntsm".to_owned(),
        commit_id: "11111111".to_owned(),
        files,
    });
    app.update(Message::DiffLoaded {
        commit_id: "11111111".to_owned(),
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
    app.update(Message::DiffLoaded {
        commit_id: "11111111".to_owned(),
        path: "src/a/very/long/directory/that/must/keep/file.rs".to_owned(),
        rows: large_diff,
        old_content: None,
        new_content: None,
    });

    let wide = screen(&app, 120, 30).join("\n");
    assert!(wide.contains("Files (focus)"));
    assert!(wide.contains("Diff · src/a/very/long"));
    assert!(wide.contains("●"));
    assert!(wide.contains('!'));

    let threshold = screen(&app, 72, 15).join("\n");
    assert!(threshold.contains("Files (focus)"));
    assert!(threshold.contains("Diff ·"));

    app.update(Message::Resize {
        width: 60,
        height: 10,
    });
    let narrow_files = screen(&app, 60, 10).join("\n");
    assert!(narrow_files.contains("Files (focus)"));
    assert!(!narrow_files.contains("Diff ·"));
    assert!(narrow_files.contains("file.rs"));

    app.update(Message::Key(Key::Tab));
    let narrow_diff = screen(&app, 60, 10).join("\n");
    assert!(narrow_diff.contains("Diff ·"));
    assert!(!narrow_diff.contains("Files (focus)"));

    let minimum = screen(&app, 40, 6).join("\n");
    assert!(minimum.contains("Progressive review"));
    let too_small = screen(&app, 39, 5);
    assert_eq!(too_small[0].trim_end(), "Terminal is too small");
    assert_eq!(too_small[1].trim_end(), "Minimum: 40x6");
    assert_eq!(too_small[2].trim_end(), "q quit");

    app.update(Message::Key(Key::Tab));
    app.update(Message::Key(Key::Down));
    app.update(Message::Key(Key::Tab));
    app.update(Message::Key(Key::Visual));
    assert!(
        screen(&app, 80, 10)
            .join("\n")
            .contains("This diff row cannot be selected")
    );
}
