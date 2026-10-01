use super::*;

#[test]
fn lines_keep_their_terminators_and_a_last_line_without_one() {
    let lines = Lines::new(b"one\ntwo\r\nthree");

    assert_eq!(lines.len(), 3);
    assert_eq!(lines.line(1), Some(&b"two\r\n"[..]));
    assert_eq!(lines.line(2), Some(&b"three"[..]));
    assert_eq!(lines.line(3), None);
}

#[test]
fn changes_report_replaced_line_ranges_on_both_sides() {
    assert_eq!(
        changes(b"a\nb\nc\nd\n", b"a\nB\nc\nd\ne\n"),
        vec![
            Change {
                before: 1..2,
                after: 1..2,
            },
            Change {
                before: 4..4,
                after: 4..5,
            },
        ]
    );
}

#[test]
fn ranges_overlap_only_when_they_share_a_line() {
    assert!(overlaps(&(2..5), &(4..6)));
    assert!(!overlaps(&(2..5), &(5..6)));
}

#[test]
fn an_empty_range_meets_the_ranges_on_both_of_its_sides() {
    assert!(overlaps(&(5..5), &(2..5)));
    assert!(overlaps(&(5..5), &(5..6)));
    assert!(!overlaps(&(5..5), &(6..8)));
}

#[test]
fn translation_shifts_past_earlier_edits_and_refuses_touched_ranges() {
    let edits = [(1..2, 1..4), (8..9, 10..10)];
    let pairs = || edits.iter().map(|(from, to)| (from, to));

    assert_eq!(translate(pairs(), &(4..6)), Some(6..8));
    assert_eq!(translate(pairs(), &(0..1)), Some(0..1));
    assert_eq!(translate(pairs(), &(1..3)), None);
}

#[test]
fn a_line_moves_past_changes_before_it_and_vanishes_in_one() {
    let changes = [
        Change {
            before: 1..2,
            after: 1..4,
        },
        Change {
            before: 5..5,
            after: 7..9,
        },
    ];

    assert_eq!(after_line(&changes, 0), Some(0));
    assert_eq!(after_line(&changes, 1), None);
    assert_eq!(after_line(&changes, 4), Some(6));
    // An insertion right before a line only shifts it.
    assert_eq!(after_line(&changes, 5), Some(9));
}
