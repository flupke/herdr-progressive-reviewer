use pr_app::review::{ReviewState, ReviewStatus, ReviewWarning};
use pr_app::ui::{Action, Key, Message, ReviewApp, ReviewFile};
use pr_core::diff::{DiffRow, NoticeKind};
use pr_core::herdr::InsertResult;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

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
    });
    assert!(!screen(&app, 80, 12).join("\n").contains("stale result"));
    app.update(Message::DiffLoaded {
        commit_id: "11111111".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: rows(),
    });

    app.update(Message::Key(Key::Enter));
    app.update(Message::Key(Key::Down));
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
    });

    let wide = screen(&app, 120, 30).join("\n");
    assert!(wide.contains("Files (focus)"));
    assert!(wide.contains("Diff · src/a/very/long"));
    assert!(wide.contains("●"));
    assert!(wide.contains('!'));

    let threshold = screen(&app, 72, 15).join("\n");
    assert!(threshold.contains("Files (focus)"));
    assert!(threshold.contains("Diff ·"));

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
    app.update(Message::Key(Key::Enter));
    app.update(Message::Key(Key::Visual));
    assert!(
        screen(&app, 80, 10)
            .join("\n")
            .contains("This diff row cannot be selected")
    );
}
