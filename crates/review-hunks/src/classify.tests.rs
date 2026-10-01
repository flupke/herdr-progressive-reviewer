use review_repository::diff::parse_file_diff;
use review_repository::repository::ChangedFile;

use super::*;
use crate::hunks::LineCount;
use crate::unified_diff;

/// Twenty numbered lines with some of them rewritten.
fn text(edits: &[(u32, &str)]) -> Vec<u8> {
    (1..=20)
        .map(|line| {
            edits
                .iter()
                .find(|(edited, _)| *edited == line)
                .map_or_else(|| format!("{line}\n"), |(_, text)| format!("{text}\n"))
        })
        .collect::<String>()
        .into_bytes()
}

fn hunks(base: &[u8], reviewed: &[u8], current: &[u8]) -> FileHunks {
    let open = parse_file_diff(
        &unified_diff("f.rs", reviewed, current),
        &ChangedFile::modified("f.rs"),
    );
    HunkReview::new(base, reviewed, current).hunks(&open)
}

fn span(old: Range<u32>, new: Range<u32>) -> HunkSpan {
    HunkSpan { old, new }
}

fn open(hunks: &FileHunks) -> Vec<(Range<u32>, bool)> {
    hunks
        .open
        .iter()
        .map(|hunk| (hunk.span.new.clone(), hunk.since_review))
        .collect()
}

fn reviewed(hunks: &FileHunks) -> Vec<HunkSpan> {
    hunks
        .reviewed
        .iter()
        .map(|hunk| hunk.span.clone())
        .collect()
}

#[test]
fn accepted_hunks_leave_the_open_diff_and_stay_listed_as_reviewed() {
    let current = text(&[(2, "two"), (10, "ten"), (18, "eighteen")]);

    let hunks = hunks(&text(&[]), &text(&[(2, "two"), (10, "ten")]), &current);

    assert_eq!(open(&hunks), [(17..18, false)]);
    assert_eq!(reviewed(&hunks), [span(1..2, 1..2), span(9..10, 9..10)]);
    assert_eq!(
        hunks.reviewed[0].rows,
        [
            DiffRow::Delete {
                old_line: 2,
                text: "-2".into(),
            },
            DiffRow::Add {
                new_line: 2,
                text: "+two".into(),
            },
        ]
    );
    assert_eq!(
        hunks.count(),
        // Each hunk replaces one line: two changed lines apiece.
        Some(LineCount {
            reviewed: 4,
            total: 6
        })
    );
}

#[test]
fn a_reviewed_hunk_edited_again_shows_only_the_change_since_review() {
    let current = text(&[(2, "two!"), (10, "ten"), (18, "eighteen")]);

    let hunks = hunks(&text(&[]), &text(&[(2, "two"), (10, "ten")]), &current);

    assert_eq!(open(&hunks), [(1..2, true), (17..18, false)]);
    assert_eq!(reviewed(&hunks), [span(9..10, 9..10)]);
}

#[test]
fn a_new_change_next_to_a_reviewed_one_is_not_a_change_since_review() {
    let reviewed_text = text(&[(10, "ten")]);
    let current = text(&[(10, "ten"), (13, "thirteen")]);

    let hunks = hunks(&text(&[]), &reviewed_text, &current);

    assert_eq!(open(&hunks), [(12..13, false)]);
    assert_eq!(reviewed(&hunks), [span(9..10, 9..10)]);
}

#[test]
fn nearby_reviewed_changes_join_into_one_hunk_with_the_lines_between() {
    let current = text(&[(10, "ten"), (12, "twelve")]);

    let hunks = hunks(&text(&[]), &current, &current);

    assert_eq!(reviewed(&hunks), [span(9..12, 9..12)]);
    assert_eq!(
        hunks.reviewed[0].rows[2],
        DiffRow::Context {
            old_line: 11,
            new_line: 11,
            text: " 11".into(),
        }
    );
}

#[test]
fn reviewed_changes_with_an_open_change_between_them_stay_apart() {
    let reviewed_text = text(&[(10, "ten"), (14, "fourteen")]);
    let current = text(&[(10, "ten"), (12, "twelve"), (14, "fourteen")]);

    let hunks = hunks(&text(&[]), &reviewed_text, &current);

    assert_eq!(open(&hunks), [(11..12, false)]);
    assert_eq!(reviewed(&hunks), [span(9..10, 9..10), span(13..14, 13..14)]);
}

#[test]
fn reviewed_hunks_sit_on_the_current_lines_past_open_changes() {
    let reviewed_text = text(&[(10, "ten")]);
    let current = format!("zero\n{}", String::from_utf8(text(&[(10, "ten")])).unwrap());

    let hunks = hunks(&text(&[]), &reviewed_text, current.as_bytes());

    assert_eq!(reviewed(&hunks), [span(9..10, 10..11)]);
}

#[test]
fn a_reviewed_change_between_two_new_edits_of_one_hunk_is_not_rewritten() {
    let reviewed_text = text(&[(11, "eleven")]);
    let current = text(&[(9, "nine"), (11, "eleven"), (13, "thirteen")]);

    let hunks = hunks(&text(&[]), &reviewed_text, &current);

    assert_eq!(open(&hunks), [(8..13, false)]);
}
