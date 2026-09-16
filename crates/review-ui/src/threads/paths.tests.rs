use super::*;

#[test]
fn a_copy_does_not_take_the_sources_thread_badge_conversation_or_navigation() {
    let mut ui = ThreadUi::new(120);
    let mut copy = FileSummary::new("src/copy.rs", ReviewStatus::Unreviewed);
    copy.file.change = review_repository::repository::ChangeKind::Added;
    copy.file.old_path =
        review_repository::repository::ChangedFile::modified("src/lib.rs").old_path;
    publish_repository(
        &mut ui.app,
        ReviewCheckpoint::new("change", "now"),
        "Thread navigation".into(),
        vec![copy, FileSummary::new("src/lib.rs", ReviewStatus::Reviewed)],
    );
    for (path, owns_thread) in [("src/copy.rs", false), ("src/lib.rs", true)] {
        ui.app
            .publish(ui_events::FileSelectionRequested { path: path.into() });
        ui.app.publish(ui_events::DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "now"),
            path: path.into(),
            rows: vec![DiffRow::Add {
                new_line: 1,
                text: "+original".into(),
            }],
            old_content: None,
            new_content: Some(b"original\n".to_vec()),
        });
        let text = ui.text();
        assert_eq!(text.contains("Explain this branch"), owns_thread, "{text}");
        for (file, badge) in [("copy.rs", false), ("lib.rs", true)] {
            let row = text
                .lines()
                .map(|row| row.chars().take(34).collect::<String>())
                .find(|row| row.contains(file))
                .unwrap();
            assert_eq!(row.contains("💬"), badge, "{row}");
        }
    }
    ui.key(Key::Char('t'));
    assert!(ui.text().contains("File reviewed"));
    ui.key(Key::Enter);
    let actions = ui.key(Key::Char('p'));
    assert!(
        matches!(actions.first(), Some(Action::LoadSource { location, .. })
        if location.path == PathBuf::from("/repo/src/lib.rs")),
        "{actions:?}"
    );
}

#[test]
fn a_copy_does_not_inherit_threads_when_its_source_is_outside_the_diff() {
    let mut ui = ThreadUi::new(120);
    let mut copy = FileSummary::new("src/copy.rs", ReviewStatus::Unreviewed);
    copy.file.change = review_repository::repository::ChangeKind::Added;
    copy.file.old_path =
        review_repository::repository::ChangedFile::modified("src/lib.rs").old_path;
    publish_repository(
        &mut ui.app,
        ReviewCheckpoint::new("change", "now"),
        "Copy with unchanged source".into(),
        vec![copy],
    );
    ui.app.publish(ui_events::FileSelectionRequested {
        path: "src/copy.rs".into(),
    });
    let text = ui.text();
    let copy = text
        .lines()
        .find(|row| row.contains("copy.rs") && !row.contains("Diff ·"))
        .unwrap();
    assert!(!copy.contains("💬"), "{text}");
    assert!(!text.contains("Explain this branch"), "{text}");
    ui.app.publish(ui_events::FileSelectionRequested {
        path: "src/lib.rs".into(),
    });
    assert!(ui.text().contains("Explain this branch"));
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    let actions = ui.key(Key::Char('p'));
    assert!(
        matches!(actions.first(), Some(Action::LoadSource { location, .. })
        if location.path == PathBuf::from("/repo/src/lib.rs")),
        "{actions:?}"
    );
}
