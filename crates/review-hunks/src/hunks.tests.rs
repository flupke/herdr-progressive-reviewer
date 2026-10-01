use review_repository::diff::parse_file_diff;
use review_repository::repository::ChangedFile;

use super::*;
use crate::unified_diff;

fn rows(before: &[u8], after: &[u8]) -> Vec<DiffRow> {
    parse_file_diff(
        &unified_diff("f.rs", before, after),
        &ChangedFile::modified("f.rs"),
    )
}

fn span(old: Range<u32>, new: Range<u32>) -> HunkSpan {
    HunkSpan { old, new }
}

#[test]
fn a_file_nobody_reviewed_has_only_open_hunks() {
    let hunks = FileHunks::unreviewed(&rows(
        b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n",
        b"a\nB\nc\nd\ne\nf\ng\nh\ni\nJ\n",
    ));

    assert_eq!(
        hunks.open,
        [1..2, 9..10]
            .into_iter()
            .map(|lines| OpenHunk {
                span: span(lines.clone(), lines),
                since_review: false,
                changed_lines: 2,
            })
            .collect::<Vec<_>>()
    );
    assert!(hunks.reviewed.is_empty());
    assert_eq!(hunks.count(), None);
}

#[test]
fn spans_of_pure_insertions_and_deletions_are_empty_on_one_side() {
    let spans = open_hunks(&rows(
        b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n",
        b"a\nnew\nb\nc\nd\ne\nf\ng\nh\ni\n",
    ));

    assert_eq!(
        spans
            .iter()
            .map(|hunk| (hunk.span.clone(), hunk.changed_lines))
            .collect::<Vec<_>>(),
        [(span(1..1, 1..2), 1), (span(9..10, 10..10), 1)]
    );
}

#[test]
fn unchanged_lines_between_changes_belong_to_the_span() {
    let spans = open_hunks(&rows(b"a\nb\nc\nd\n", b"A\nb\nc\nD\n"));

    assert_eq!(
        spans
            .iter()
            .map(|hunk| (hunk.span.clone(), hunk.changed_lines))
            .collect::<Vec<_>>(),
        [(span(0..4, 0..4), 4)]
    );
}

#[test]
fn a_diff_with_notices_has_no_hunks_to_review_one_by_one() {
    let binary = [DiffRow::Notice {
        kind: review_repository::diff::NoticeKind::Binary,
        text: "Binary file".into(),
    }];

    assert_eq!(FileHunks::unreviewed(&binary), FileHunks::default());
}

#[test]
fn a_partly_reviewed_share_is_neither_none_nor_all() {
    let count = |reviewed, total| LineCount { reviewed, total }.percent();

    assert_eq!(count(1, 1000), 1);
    assert_eq!(count(999, 1000), 99);
    assert_eq!(count(3, 10), 30);
    assert_eq!(count(0, 10), 0);
    assert_eq!(count(10, 10), 100);
}
