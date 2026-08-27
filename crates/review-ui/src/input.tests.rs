use std::path::PathBuf;

use review_lsp::{Operation, SourceLocation};
use review_repository::diff::DiffRow;
use review_state::ReviewStatus;

use crate::app::{
    Action, ContextMenu, DragState, Focus, LocationList, Message, ReviewApp, ReviewFile, Selection,
};

fn app_with_loaded_diff(lines: usize) -> ReviewApp {
    let mut app = ReviewApp {
        width: 100,
        height: 20,
        file_width: Some(30),
        focus: Focus::Diff,
        ..ReviewApp::default()
    };
    app.update(Message::FilesLoaded {
        change_id: "change".to_owned(),
        commit_id: "commit".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: (1..=lines)
            .map(|line| DiffRow::Add {
                new_line: u32::try_from(line).unwrap(),
                text: format!("+line {line}"),
            })
            .collect(),
        old_content: Some(b"line\n".to_vec()),
        new_content: Some(b"line\n".to_vec()),
    });
    app
}

fn source_location(line: u32) -> SourceLocation {
    SourceLocation {
        path: PathBuf::from("/outside.rs"),
        line,
        byte_column: 0,
        end_line: line,
        end_byte_column: 0,
    }
}

#[test]
fn navigation_updates_only_an_open_selection() {
    let mut app = app_with_loaded_diff(40);
    app.selection = Some(Selection {
        anchor: 1,
        cursor: 1,
        fixed: false,
    });

    assert_eq!(app.navigate_to(1), Action::None);
    assert_eq!(app.selection.unwrap().cursor, 1);

    app.selection = Some(Selection {
        anchor: 8,
        cursor: 8,
        fixed: true,
    });
    assert_eq!(app.navigate_to(0), Action::None);
    assert_eq!(app.selection.unwrap().cursor, 8);
}

#[test]
fn navigation_normalizes_columns_only_in_the_diff_pane() {
    let mut app = app_with_loaded_diff(2);
    app.update(Message::DiffLoaded {
        commit_id: "commit".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: " éx".to_owned(),
        }],
        old_content: Some("éx\n".as_bytes().to_vec()),
        new_content: Some("éx\n".as_bytes().to_vec()),
    });
    app.files[0].column = 1;

    app.focus = Focus::Diff;
    app.navigate_to(0);
    assert_eq!(app.files[0].column, 0);

    app.files[0].column = 1;
    app.focus = Focus::Files;
    app.navigate_to(0);
    assert_eq!(app.files[0].column, 1);
}

#[test]
fn leaving_a_temporary_file_by_click_removes_it_and_keeps_the_clicked_file_selected() {
    let mut temporary = ReviewFile::new("temporary.rs", ReviewStatus::Unreviewed);
    temporary.temporary = true;
    let mut app = ReviewApp {
        width: 100,
        height: 20,
        file_width: Some(30),
        files: vec![
            temporary,
            ReviewFile::new("target.rs", ReviewStatus::Unreviewed),
        ],
        ..ReviewApp::default()
    };
    app.rebuild_file_tree();
    let target_row = (0..app.file_tree.visible_file_count())
        .find(|row| app.file_tree.visible_file_at(*row) == Some(1))
        .unwrap();

    app.file_click_without_history(
        app.layout(),
        2,
        u16::try_from(target_row).unwrap() + 2,
        false,
    );

    assert_eq!(app.files.len(), 1);
    assert_eq!(app.selected_file, 0);
    assert_eq!(app.files[0].path, "target.rs");
}

#[test]
fn inserting_a_clicked_path_is_safe_when_it_removes_a_temporary_file() {
    let mut temporary = ReviewFile::new("temporary.rs", ReviewStatus::Unreviewed);
    temporary.temporary = true;
    let mut app = ReviewApp {
        width: 100,
        height: 20,
        file_width: Some(30),
        files: vec![
            temporary,
            ReviewFile::new("target.rs", ReviewStatus::Unreviewed),
        ],
        ..ReviewApp::default()
    };
    app.rebuild_file_tree();
    let target_row = (0..app.file_tree.visible_file_count())
        .find(|row| app.file_tree.visible_file_at(*row) == Some(1))
        .unwrap();

    assert!(matches!(
        app.file_click_without_history(
            app.layout(),
            2,
            u16::try_from(target_row).unwrap() + 2,
            true,
        ),
        Action::Output { text, .. } if text == "target.rs"
    ));
    assert_eq!(app.files.len(), 1);
    assert_eq!(app.selected_file, 0);
}

#[test]
fn right_click_rejects_each_invalid_area_independently() {
    let mut app = app_with_loaded_diff(3);
    app.context_menu = Some(ContextMenu {
        column: 40,
        row: 3,
        selected: 0,
        enabled: true,
    });
    app.preview = Some(ReviewFile::new("preview.rs", ReviewStatus::Unreviewed));
    app.mouse_right_click(40, 3);
    assert!(app.context_menu.is_none());

    app.preview = None;
    app.focus = Focus::Files;
    app.context_menu = Some(ContextMenu {
        column: 2,
        row: 3,
        selected: 0,
        enabled: true,
    });
    app.mouse_right_click(2, 3);
    assert!(app.context_menu.is_none());

    app.focus = Focus::Diff;
    app.context_menu = Some(ContextMenu {
        column: 40,
        row: 1,
        selected: 0,
        enabled: true,
    });
    app.mouse_right_click(40, 1);
    assert!(app.context_menu.is_none());
}

#[test]
fn double_click_uses_the_scrolled_location_row() {
    let mut app = app_with_loaded_diff(3);
    app.focus = Focus::Files;
    app.repository_root = PathBuf::from("/repository");
    app.locations = Some(LocationList {
        operation: Operation::References,
        locations: (0..6).map(source_location).collect(),
        selected: 0,
        scroll: 1,
        origin_file: 0,
    });

    assert!(matches!(
        app.mouse_double_click(2, 5),
        Action::LoadSource { location, .. } if location.line == 4
    ));
}

#[test]
fn resize_drag_clamps_at_the_right_pane_minimum_width() {
    let mut app = app_with_loaded_diff(3);
    app.drag = DragState::Resize { moved: false };

    app.mouse_drag(95, 3);

    assert_eq!(app.file_width, Some(84));
}
