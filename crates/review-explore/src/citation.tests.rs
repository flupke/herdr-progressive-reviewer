use review_repository::diff::{DiffRow, NoticeKind};
use review_repository::repository::RepoPath;
use review_source::SourceLineRange;
use test_case::test_case;

use super::{CitedLines, Uncitable};
use crate::{CodeLocation, SourceSide};

const OLD: &str = "a\nb\nc\nd\ne\nf\n";
const NEW: &str = "a\nb\nC\nd\ne\nf\ng\n";

/// The diff of OLD to NEW, with one line of context: `c` became `C`, `g` was added.
fn diff() -> Vec<DiffRow> {
    let context = |old_line, new_line, text: &str| DiffRow::Context {
        old_line,
        new_line,
        text: format!(" {text}"),
    };
    vec![
        DiffRow::Hunk {
            old_start: 2,
            old_count: 3,
            new_start: 2,
            new_count: 3,
        },
        context(2, 2, "b"),
        DiffRow::Delete {
            old_line: 3,
            text: "-c".into(),
        },
        DiffRow::Add {
            new_line: 3,
            text: "+C".into(),
        },
        context(4, 4, "d"),
        DiffRow::Hunk {
            old_start: 6,
            old_count: 1,
            new_start: 6,
            new_count: 2,
        },
        context(6, 6, "f"),
        DiffRow::Add {
            new_line: 7,
            text: "+g".into(),
        },
    ]
}

/// Each row as its old line, its new line and its text.
fn shown(rows: &[DiffRow]) -> Vec<(Option<u32>, Option<u32>, &str)> {
    rows.iter()
        .map(|row| match row {
            DiffRow::Context {
                old_line,
                new_line,
                text,
            } => (Some(*old_line), Some(*new_line), text.as_str()),
            DiffRow::Delete { old_line, text } => (Some(*old_line), None, text.as_str()),
            DiffRow::Add { new_line, text } => (None, Some(*new_line), text.as_str()),
            other => panic!("unexpected row {other:?}"),
        })
        .collect()
}

fn location(side: SourceSide, first_line: u32, last_line: u32) -> CodeLocation {
    CodeLocation {
        path: RepoPath::from_bytes("notes.rs"),
        side,
        lines: Some(SourceLineRange {
            first_line,
            last_line,
        }),
    }
}

fn cite(side: SourceSide, first_line: u32, last_line: u32) -> Result<CitedLines, Uncitable> {
    CitedLines::in_diff(
        &location(side, first_line, last_line),
        &diff(),
        Some(OLD.into()),
        Some(NEW.into()),
    )
}

#[test]
fn cited_new_lines_show_the_unchanged_lines_before_a_hunk_and_its_removed_lines() {
    let lines = cite(SourceSide::New, 1, 4).unwrap();
    assert_eq!(
        shown(&lines.rows),
        [
            (Some(1), Some(1), " a"),
            (Some(2), Some(2), " b"),
            (Some(3), None, "-c"),
            (None, Some(3), "+C"),
            (Some(4), Some(4), " d"),
        ]
    );
    assert_eq!(lines.path, "notes.rs");
    assert_eq!(lines.old_content.as_deref(), Some(OLD.as_bytes()));
    assert_eq!(lines.new_content.as_deref(), Some(NEW.as_bytes()));
}

#[test]
fn cited_new_lines_between_hunks_and_after_the_last_one_are_numbered_on_both_sides() {
    let lines = cite(SourceSide::New, 5, 7).unwrap();
    assert_eq!(
        shown(&lines.rows),
        [
            (Some(5), Some(5), " e"),
            (Some(6), Some(6), " f"),
            (None, Some(7), "+g"),
        ]
    );
}

#[test]
fn cited_old_lines_show_removed_lines_without_the_added_ones_after_them() {
    assert_eq!(
        shown(&cite(SourceSide::Old, 1, 3).unwrap().rows),
        [
            (Some(1), Some(1), " a"),
            (Some(2), Some(2), " b"),
            (Some(3), None, "-c"),
        ]
    );
}

#[test_case(SourceSide::New, 8, 8; "past the end of the new side")]
#[test_case(SourceSide::Old, 7, 7; "past the end of the old side")]
#[test_case(SourceSide::New, 4, 2; "reversed")]
fn lines_the_side_does_not_have_are_not_cited(side: SourceSide, first: u32, last: u32) {
    assert_eq!(cite(side, first, last), Err(Uncitable::Range));
}

#[test]
fn a_side_without_text_cites_no_lines() {
    let lines = CitedLines::in_diff(
        &location(SourceSide::Old, 1, 1),
        &diff(),
        None,
        Some(NEW.into()),
    );
    assert_eq!(lines, Err(Uncitable::Range));
}

#[test]
fn a_change_without_a_text_diff_cites_no_lines() {
    let notice = DiffRow::Notice {
        kind: NoticeKind::Binary,
        text: "Binary file; text diff is unavailable".into(),
    };
    let lines = CitedLines::in_diff(
        &location(SourceSide::New, 1, 1),
        &[notice],
        Some(OLD.into()),
        Some(NEW.into()),
    );
    assert_eq!(lines, Err(Uncitable::NoTextDiff(NoticeKind::Binary)));
}
