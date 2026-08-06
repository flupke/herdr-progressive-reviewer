use ratatui::style::Color;
use review_repository::diff::DiffRow;

use super::{DiffPresentation, PresentedRow};
use crate::highlight::{HighlightedDiff, HighlightedFile, HighlightedRow, Token};

#[test]
fn added_file_shows_only_file_contents() {
    let highlighted = HighlightedDiff {
        rows: vec![
            HighlightedRow {
                diff: DiffRow::Meta {
                    text: "new file mode 100644".to_owned(),
                },
                tokens: Vec::new(),
            },
            HighlightedRow {
                diff: DiffRow::Hunk {
                    old_start: 0,
                    old_count: 0,
                    new_start: 1,
                    new_count: 1,
                },
                tokens: Vec::new(),
            },
            HighlightedRow {
                diff: DiffRow::Add {
                    new_line: 1,
                    text: "+fn main() {}".to_owned(),
                },
                tokens: vec![Token {
                    text: "fn main() {}".to_owned(),
                    color: Color::White,
                }],
            },
        ],
        file: None,
    };

    let presentation = DiffPresentation::new(highlighted);

    assert_eq!(presentation.rows.len(), 1);
    assert!(matches!(presentation.rows[0], PresentedRow::Diff { .. }));
}

#[test]
fn hides_headers_and_expands_gaps_at_each_file_boundary() {
    let row = |diff| HighlightedRow {
        diff,
        tokens: Vec::new(),
    };
    let mut presentation = DiffPresentation::new(HighlightedDiff {
        rows: vec![
            row(DiffRow::FileHeader {
                old_path: None,
                new_path: None,
                text: "diff --git a/file b/file".to_owned(),
            }),
            row(DiffRow::Hunk {
                old_start: 2,
                old_count: 1,
                new_start: 2,
                new_count: 1,
            }),
            row(DiffRow::Context {
                old_line: 2,
                new_line: 2,
                text: " second".to_owned(),
            }),
            row(DiffRow::Hunk {
                old_start: 4,
                old_count: 1,
                new_start: 4,
                new_count: 1,
            }),
            row(DiffRow::Context {
                old_line: 4,
                new_line: 4,
                text: " fourth".to_owned(),
            }),
        ],
        file: Some(HighlightedFile::AfterChange(
            (1..=5)
                .map(|line| {
                    vec![Token {
                        text: format!("line {line}"),
                        color: Color::White,
                    }]
                })
                .collect(),
        )),
    });

    assert!(matches!(presentation.rows[0], PresentedRow::Gap { .. }));
    assert!(matches!(presentation.rows[2], PresentedRow::Gap { .. }));
    assert!(matches!(presentation.rows[4], PresentedRow::Gap { .. }));
    assert!(presentation.expand(0));
    assert_eq!(presentation.rows.len(), 5);
    assert!(matches!(
        presentation.rows[0],
        PresentedRow::Expanded { line: 1, .. }
    ));

    assert!(presentation.expand_all());
    assert!(
        presentation
            .rows
            .iter()
            .all(|row| !matches!(row, PresentedRow::Gap { .. }))
    );
    assert!(presentation.contract_all());
    assert_eq!(
        presentation
            .rows
            .iter()
            .filter(|row| matches!(row, PresentedRow::Gap { .. }))
            .count(),
        3
    );
}

#[test]
fn file_view_restores_the_diff_rows() {
    let diff_row = PresentedRow::Gap {
        start: 1,
        lines: Vec::new(),
    };
    let mut presentation = DiffPresentation {
        rows: vec![diff_row.clone()],
        file_rows: Some(vec![PresentedRow::Expanded {
            line: 1,
            tokens: vec![Token {
                text: "fn main() {}".to_owned(),
                color: Color::Blue,
            }],
        }]),
        ..DiffPresentation::default()
    };

    assert!(presentation.show_file());
    assert!(presentation.is_file_view());
    assert!(matches!(
        presentation.rows[0],
        PresentedRow::Expanded { line: 1, .. }
    ));
    assert!(presentation.show_diff());
    assert_eq!(presentation.rows, vec![diff_row]);
}
