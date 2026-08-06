use review_repository::diff::DiffRow;

use super::SyntaxHighlighter;
use crate::theme::Theme;

#[test]
fn uses_syntax_state_from_lines_before_the_hunk() {
    let rows = vec![DiffRow::Add {
        new_line: 2,
        text: "+let answer = 42;".to_owned(),
    }];
    let highlighted = SyntaxHighlighter::new(Theme::default()).highlight(
        "src/lib.rs",
        rows,
        None,
        Some(b"/* comment\nlet answer = 42;\n*/\n"),
    );

    assert_eq!(
        highlighted.rows[0]
            .tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect::<String>(),
        "let answer = 42;"
    );
    assert_eq!(highlighted.rows[0].tokens.len(), 1);
}

#[test]
fn preserves_tabs_for_source_coordinates() {
    let highlighted = SyntaxHighlighter::new(Theme::default()).highlight(
        "Makefile",
        vec![DiffRow::Add {
            new_line: 1,
            text: "+\tcargo build".to_owned(),
        }],
        None,
        Some(b"\tcargo build\n"),
    );

    assert_eq!(
        highlighted.rows[0]
            .tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect::<String>(),
        "\tcargo build"
    );
}

#[test]
fn does_not_show_before_change_contents_when_after_change_content_is_binary() {
    let highlighted = SyntaxHighlighter::new(Theme::default()).highlight(
        "src/lib.rs",
        Vec::new(),
        Some(b"old contents\n"),
        Some(b"\xff"),
    );

    assert!(highlighted.file.is_none());
}
