use super::*;

#[test]
fn finds_a_nested_rust_workspace() {
    let directory = tempfile::tempdir().unwrap();
    let crates = directory.path().join("crates");
    std::fs::create_dir(&crates).unwrap();
    std::fs::write(crates.join("Cargo.toml"), "[workspace]\n").unwrap();

    assert_eq!(rust_project_root(directory.path()), Some(crates));
}

#[test]
fn only_rust_documents_are_opened_for_lsp() {
    assert_eq!(
        rust_document_path(std::path::Path::new("/repo"), "src/lib.rs"),
        Some(PathBuf::from("/repo/src/lib.rs"))
    );
    assert_eq!(
        rust_document_path(std::path::Path::new("/repo"), "README.md"),
        None
    );
}

#[test]
fn references_are_restricted_to_the_rust_project() {
    let location = |path| review_lsp::SourceLocation {
        path: PathBuf::from(path),
        line: 0,
        byte_column: 0,
        end_line: 0,
        end_byte_column: 1,
    };
    let locations = vec![
        location("/repo/crates/src/lib.rs"),
        location("/repo/src/lib.rs"),
        location("/dependency/src/lib.rs"),
    ];

    assert_eq!(
        review_lsp::Operation::References
            .filter_locations(std::path::Path::new("/repo/crates"), locations)
            .into_iter()
            .map(|location| location.path)
            .collect::<Vec<_>>(),
        vec![PathBuf::from("/repo/crates/src/lib.rs")]
    );
}

#[test]
fn modified_mouse_inputs_reuse_existing_actions() {
    assert_eq!(
        normalize_key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT)),
        Some(Key::Char('V'))
    );
    assert_eq!(
        normalize_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE)),
        Some(Key::Char('l'))
    );
    assert_eq!(
        normalize_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)),
        Some(Key::Char('c'))
    );
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 4,
            row: 5,
            modifiers: KeyModifiers::SHIFT,
        }),
        Some(Message::MouseScroll {
            column: 4,
            row: 5,
            delta: 6,
        })
    );
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 4,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
        Some(Message::MouseScroll {
            column: 4,
            row: 5,
            delta: 3,
        })
    );
    for (kind, modifiers) in [
        (MouseEventKind::Down(MouseButton::Left), KeyModifiers::SHIFT),
        (
            MouseEventKind::Down(MouseButton::Middle),
            KeyModifiers::NONE,
        ),
    ] {
        assert_eq!(
            normalize_mouse(MouseEvent {
                kind,
                column: 4,
                row: 5,
                modifiers,
            }),
            Some(Message::MouseClick {
                column: 4,
                row: 5,
                insert_path: true,
            })
        );
    }
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: 5,
            modifiers: KeyModifiers::CONTROL,
        }),
        Some(Message::MouseControlClick { column: 4, row: 5 })
    );
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 40,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
        Some(Message::MouseDrag { column: 40, row: 5 })
    );
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 40,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
        Some(Message::MouseRelease)
    );
}

#[test]
fn consecutive_plain_clicks_at_one_position_become_a_double_click() {
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 4,
        row: 5,
        modifiers: KeyModifiers::NONE,
    };
    let start = Instant::now();
    let mut clicks = MouseClicks::default();

    assert!(matches!(
        clicks.normalize_at(click, start),
        Some(Message::MouseClick { .. })
    ));
    let adjacent = MouseEvent { column: 5, ..click };
    assert_eq!(
        clicks.normalize_at(adjacent, start + Duration::from_millis(400)),
        Some(Message::MouseDoubleClick { column: 5, row: 5 })
    );
    assert!(matches!(
        clicks.normalize_at(click, start + Duration::from_millis(450)),
        Some(Message::MouseClick { .. })
    ));
    let modified = MouseEvent {
        modifiers: KeyModifiers::CONTROL,
        ..click
    };
    clicks.normalize_at(modified, start + Duration::from_millis(460));
    assert!(matches!(
        clicks.normalize_at(click, start + Duration::from_millis(470)),
        Some(Message::MouseClick { .. })
    ));
}
