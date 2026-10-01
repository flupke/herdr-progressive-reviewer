use std::ops::Range;

use super::*;

const BASE: &[u8] = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
const CURRENT: &[u8] = b"a\nB\nc\nd\ne\nf\ng\nh\nI\nj\n";

fn span(old: Range<u32>, new: Range<u32>) -> HunkSpan {
    HunkSpan { old, new }
}

#[test]
fn accepting_an_open_hunk_gives_the_reviewed_version_its_current_lines() {
    let review = HunkReview::new(BASE, BASE, CURRENT);

    assert_eq!(
        review.review(&span(1..2, 1..2)),
        Some(ReviewedVersion::Partial(
            b"a\nB\nc\nd\ne\nf\ng\nh\ni\nj\n".to_vec()
        ))
    );
}

#[test]
fn accepting_the_last_open_hunk_reviews_the_whole_file() {
    let reviewed = b"a\nB\nc\nd\ne\nf\ng\nh\ni\nj\n";
    let review = HunkReview::new(BASE, reviewed, CURRENT);

    assert_eq!(
        review.review(&span(8..9, 8..9)),
        Some(ReviewedVersion::Current)
    );
}

#[test]
fn reopening_a_reviewed_hunk_gives_it_back_its_base_lines() {
    let reviewed = b"a\nB\nc\nd\ne\nf\ng\nh\ni\nj\n";
    let review = HunkReview::new(BASE, reviewed, CURRENT);

    assert_eq!(
        review.unreview(&span(1..2, 1..2)),
        Some(ReviewedVersion::Base)
    );
}

#[test]
fn reopening_maps_the_hunk_past_open_hunks_of_another_length() {
    let current = b"a\nx\ny\nz\nb\nc\nd\ne\nf\ng\nh\nI\nj\n";
    let reviewed = b"a\nb\nc\nd\ne\nf\ng\nh\nI\nj\n";
    let review = HunkReview::new(BASE, reviewed, current);

    assert_eq!(
        review.unreview(&span(8..9, 11..12)),
        Some(ReviewedVersion::Base)
    );
}

#[test]
fn a_hunk_the_open_diff_touches_cannot_be_reopened() {
    let reviewed = b"a\nB\nc\nd\ne\nf\ng\nh\ni\nj\n";
    let review = HunkReview::new(BASE, reviewed, b"a\nBB\nc\nd\ne\nf\ng\nh\nI\nj\n");

    assert_eq!(review.unreview(&span(1..2, 1..2)), None);
}

#[test]
fn a_reviewed_version_on_an_unchanged_base_is_kept() {
    assert_eq!(replay(BASE, CURRENT, BASE), CURRENT);
}

#[test]
fn reviewed_changes_land_on_a_new_base_beside_upstream_changes() {
    let new_base = b"top\na\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";

    assert_eq!(
        replay(BASE, b"a\nB\nc\nd\ne\nf\ng\nh\ni\nj\n", new_base),
        b"top\na\nB\nc\nd\ne\nf\ng\nh\ni\nj\n"
    );
}

#[test]
fn a_reviewed_change_that_meets_an_upstream_change_is_dropped() {
    let new_base = b"a\nb-upstream\nc\nd\ne\nf\ng\nh\ni\nj\n";

    assert_eq!(
        replay(BASE, b"a\nB\nc\nd\ne\nf\ng\nh\nI\nj\n", new_base),
        b"a\nb-upstream\nc\nd\ne\nf\ng\nh\nI\nj\n"
    );
}

#[test]
fn several_open_hunks_are_accepted_at_once() {
    let review = HunkReview::new(BASE, BASE, CURRENT);

    assert_eq!(
        review.review_all(&[span(8..9, 8..9), span(1..2, 1..2)]),
        Some(ReviewedVersion::Current)
    );
}

#[test]
fn overlapping_hunks_are_not_accepted_together() {
    let review = HunkReview::new(BASE, BASE, CURRENT);

    assert_eq!(
        review.review_all(&[span(1..3, 1..3), span(2..4, 2..4)]),
        None
    );
}

#[test]
fn an_open_hunk_changes_base_lines_and_current_lines() {
    let review = HunkReview::new(BASE, BASE, CURRENT);

    assert_eq!(
        review.changed_lines(&[span(1..2, 1..2)]),
        [ChangedLines {
            removed: vec![1],
            added: vec![1],
            rewrites_reviewed: false,
        }]
    );
}

#[test]
fn reviewed_lines_edited_away_are_not_base_lines() {
    // Line 2 was reviewed as "B" and is now "BB": only the new line is a
    // change from the base, and the hunk rewrites the reviewed "B".
    let reviewed = b"a\nB\nc\nd\ne\nf\ng\nh\ni\nj\n";
    let current = b"a\nBB\nc\nd\ne\nf\ng\nh\ni\nj\n";
    let review = HunkReview::new(BASE, reviewed, current);

    assert_eq!(
        review.changed_lines(&[span(1..2, 1..2)]),
        [ChangedLines {
            removed: vec![],
            added: vec![1],
            rewrites_reviewed: true,
        }]
    );
}

#[test]
fn a_base_line_after_a_reviewed_deletion_is_still_a_base_line() {
    // Removing "b" was reviewed; the edit now replaces "c", a base line.
    let review = HunkReview::new(b"a\nb\nc\n", b"a\nc\n", b"a\nX\n");

    assert_eq!(
        review.changed_lines(&[span(1..2, 1..2)]),
        [ChangedLines {
            removed: vec![2],
            added: vec![1],
            rewrites_reviewed: false,
        }]
    );
}
