use super::{LocationHistory, ReviewLocation};
use crate::presentation::PresentationLocation;

fn review_location(path: &str, cursor: usize) -> ReviewLocation {
    ReviewLocation::ReviewFile {
        path: path.to_owned(),
        cursor,
        presentation_location: None,
        column: 0,
    }
}

#[test]
fn same_line_uses_the_stable_presentation_location_when_available() {
    let plain = review_location("src/lib.rs", 3);
    let mut other_column = review_location("src/lib.rs", 3);
    let ReviewLocation::ReviewFile { column, .. } = &mut other_column else {
        unreachable!();
    };
    *column = 20;
    assert!(plain.same_line(&other_column));
    assert!(!plain.same_line(&review_location("src/lib.rs", 4)));
    assert!(!plain.same_line(&review_location("src/other.rs", 3)));

    let stable = ReviewLocation::ReviewFile {
        path: "src/lib.rs".to_owned(),
        cursor: 3,
        presentation_location: Some(PresentationLocation::NewLine(10)),
        column: 0,
    };
    let moved = ReviewLocation::ReviewFile {
        path: "src/lib.rs".to_owned(),
        cursor: 50,
        presentation_location: Some(PresentationLocation::NewLine(10)),
        column: 8,
    };
    assert!(stable.same_line(&moved));
    assert!(!stable.same_line(&plain));
}

#[test]
fn history_keeps_one_hundred_distinct_origins() {
    let mut history = LocationHistory::default();
    let target = review_location("target.rs", 0);
    for cursor in 0..102 {
        assert!(history.record_jump(review_location("src/lib.rs", cursor), &target));
    }

    assert_eq!(history.older.len(), 100);
    assert_eq!(
        history.older.first(),
        Some(&review_location("src/lib.rs", 2))
    );
    assert_eq!(
        history.older.last(),
        Some(&review_location("src/lib.rs", 101))
    );
    assert!(!history.record_jump(target.clone(), &target));
    assert_eq!(history.older.len(), 100);
}

#[test]
fn history_traversal_skips_unrestorable_locations_and_supports_forward_navigation() {
    let mut history = LocationHistory::default();
    let first = review_location("first.rs", 1);
    let skipped = review_location("skip.rs", 2);
    let current = review_location("current.rs", 3);
    history.older = vec![first.clone(), skipped];

    assert_eq!(
        history.previous(current.clone(), |location| {
            location != &review_location("skip.rs", 2)
        }),
        Some(first.clone())
    );
    assert_eq!(history.newer, vec![current.clone()]);
    assert_eq!(history.next(first, |_| true), Some(current));
}
