use review_types::MarkAuthor;

use std::ops::Range;

use super::*;
use crate::attribution::{Attribution, AuthoredLines};

fn selection(removed: &[u32], added: &[u32]) -> LineSelection {
    LineSelection {
        removed: removed.iter().copied().collect(),
        added: added.iter().copied().collect(),
    }
}

fn partial(version: Option<ReviewedVersion>) -> Reviewed {
    match version {
        Some(ReviewedVersion::Partial(reviewed)) => reviewed,
        other => panic!("expected a partial review, got {other:?}"),
    }
}

#[test]
fn accepting_some_added_lines_splits_their_hunk() {
    let (base, current) = (b"a\nb\n", b"a\nx\ny\nz\nb\n");
    let review = HunkReview::new(base, base, current);

    let reviewed = partial(review.accept_lines(&selection(&[], &[2]), &MarkAuthor::Jev));

    assert_eq!(reviewed.text, b"a\ny\nb\n");
    assert_eq!(
        open_lines(base, &reviewed.text, current),
        LineSelection {
            removed: [].into(),
            added: [1, 3].into(),
        }
    );
    assert_eq!(
        reviewed.attribution.uniform_author(),
        Some(&MarkAuthor::Jev)
    );
}

#[test]
fn a_replacement_splits_by_side() {
    let (base, current) = (b"a\nb\nc\n", b"a\nB\nc\n");
    let review = HunkReview::new(base, base, current);

    let added = partial(review.accept_lines(&selection(&[], &[1]), &MarkAuthor::Reviewer));
    assert_eq!(added.text, b"a\nb\nB\nc\n");
    assert_eq!(
        open_lines(base, &added.text, current),
        LineSelection {
            removed: [1].into(),
            added: [].into(),
        }
    );

    let removed = partial(review.accept_lines(&selection(&[1], &[]), &MarkAuthor::Reviewer));
    assert_eq!(removed.text, b"a\nc\n");
    assert_eq!(
        open_lines(base, &removed.text, current),
        LineSelection {
            removed: [].into(),
            added: [1].into(),
        }
    );
}

#[test]
fn accepting_every_open_line_reviews_the_whole_file() {
    let (base, current) = (b"a\nb\nc\n", b"a\nB\nc\n");
    let review = HunkReview::new(base, base, current);

    assert_eq!(
        review.accept_lines(&selection(&[1], &[1]), &MarkAuthor::Jev),
        Some(ReviewedVersion::Current(Attribution::uniform(
            MarkAuthor::Jev
        )))
    );
}

#[test]
fn rewritten_reviewed_lines_leave_with_their_whole_change() {
    // "B" was reviewed and is now "BB": the edit has one addressable line.
    let review = HunkReview::new(b"a\nb\nc\n", b"a\nB\nc\n", b"a\nBB\nc\n");

    assert!(matches!(
        review.accept_lines(&selection(&[], &[1]), &MarkAuthor::Reviewer),
        Some(ReviewedVersion::Current(_))
    ));
}

#[test]
fn a_selection_without_open_lines_accepts_nothing() {
    let review = HunkReview::new(b"a\nb\n", b"a\nb\n", b"a\nB\n");

    assert_eq!(
        review.accept_lines(&selection(&[0], &[0]), &MarkAuthor::Reviewer),
        None
    );
}

#[test]
fn reopening_an_added_line_puts_it_back_in_the_open_diff() {
    let (base, current) = (b"a\nb\n", b"a\nx\ny\nb\n");
    let review = HunkReview::new(base, current, current);

    let reviewed = partial(review.reopen_lines(&selection(&[], &[1])));

    assert_eq!(reviewed.text, b"a\ny\nb\n");
    assert_eq!(
        open_lines(base, &reviewed.text, current),
        LineSelection {
            removed: [].into(),
            added: [1].into(),
        }
    );
}

#[test]
fn reopening_every_reviewed_line_gives_back_the_base() {
    let (base, current) = (b"a\nb\nc\n", b"a\nc\n");
    let review = HunkReview::new(base, current, current);

    assert_eq!(
        review.reopen_lines(&selection(&[1], &[])),
        Some(ReviewedVersion::Base)
    );
}

#[test]
fn reopening_keeps_the_authors_of_the_other_lines() {
    let (base, current) = (b"a\nb\n", b"a\nx\ny\nb\n");
    let attribution = Attribution {
        default: MarkAuthor::Reviewer,
        removed: Vec::new(),
        added: vec![AuthoredLines {
            lines: 2..3,
            author: MarkAuthor::Jev,
        }],
    };
    let review = HunkReview::new(base, current, current).attributed(&attribution);

    let reviewed = partial(review.reopen_lines(&selection(&[], &[1])));

    assert_eq!(reviewed.attribution.added_by(1), &MarkAuthor::Jev);
}

#[test]
fn a_hunk_whose_picks_would_come_out_wrong_changes_whole() {
    let changes = [
        Change {
            before: 1..2,
            after: 1..3,
        },
        Change {
            before: 20..21,
            after: 21..23,
        },
    ];
    let picks = vec![
        Pick {
            removed: vec![false],
            added: vec![true, false],
        },
        Pick {
            removed: vec![false],
            added: vec![true, false],
        },
    ];

    // The first hunk cannot be split; the second can.
    let settled = settle(&changes, picks, |picks| {
        picks[0].added == [false, false] || picks[0].added == [true, true]
    })
    .expect("lines were picked");

    assert_eq!(settled[0].added, [true, true]);
    assert_eq!(settled[0].removed, [true]);
    assert_eq!(settled[1].added, [true, false]);
}

#[test]
fn changes_a_few_lines_apart_share_a_hunk() {
    let at = |before: Range<u32>| Change {
        before: before.clone(),
        after: before,
    };
    assert_eq!(
        hunks(&[at(0..1), at(5..6), at(20..21)]).collect::<Vec<_>>(),
        [0..2, 2..3]
    );
}

#[test]
fn a_split_the_new_diff_would_pair_with_an_equal_line_marks_the_whole_hunk() {
    // Accepting the first "b" alone leaves a reviewed version whose diff to
    // the current file opens the first "b" instead of the second.
    let review = HunkReview::new(b"a\n", b"a\n", b"b\nb\n");

    assert!(matches!(
        review.accept_lines(&selection(&[], &[0]), &MarkAuthor::Jev),
        Some(ReviewedVersion::Current(_))
    ));
}

#[test]
fn accepting_some_removed_lines_keeps_the_others_open() {
    let (base, current) = (b"a\nb\nc\nd\n", b"a\nd\n");
    let review = HunkReview::new(base, base, current);

    let reviewed = partial(review.accept_lines(&selection(&[2], &[]), &MarkAuthor::Jev));

    assert_eq!(reviewed.text, b"a\nb\nd\n");
    assert_eq!(
        open_lines(base, &reviewed.text, current),
        LineSelection {
            removed: [1].into(),
            added: [].into(),
        }
    );
    assert_eq!(reviewed.attribution.removed_by(2), &MarkAuthor::Jev);
}

#[test]
fn reviewed_lines_name_their_authors_in_current_numbering() {
    let (base, reviewed, current) = (b"a\nb\n", b"a\nX\nb\n", b"top\na\nX\nb\n");
    let attribution = Attribution::uniform(MarkAuthor::Jev);
    let review = HunkReview::new(base, reviewed, current).attributed(&attribution);

    let lines = review.reviewed_lines();

    assert_eq!(lines.added, [(2, MarkAuthor::Jev)].into());
    assert!(lines.removed.is_empty());
    assert_eq!(
        open_lines(base, reviewed, current),
        LineSelection {
            removed: [].into(),
            added: [0].into(),
        }
    );
}
