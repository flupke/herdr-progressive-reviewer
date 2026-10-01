use review_repository::repository::DiffStatistics;

use super::ReviewProgress;
use crate::ReviewState;

fn lines(lines_added: u64, lines_removed: u64) -> DiffStatistics {
    DiffStatistics {
        lines_added,
        lines_removed,
    }
}

#[test]
fn partly_reviewed_hunks_count_toward_progress() {
    let unreviewed = ReviewState::unreviewed(lines(10, 0), None);
    let partial = ReviewState::partially_reviewed(lines(5, 5), None);
    let progress: ReviewProgress = [(lines(10, 0), &unreviewed), (lines(20, 10), &partial)]
        .into_iter()
        .collect();

    assert_eq!(progress.percent(), 50);
    assert_eq!(progress.filled(12), 6);
}

#[test]
fn a_file_left_to_review_keeps_progress_below_full() {
    let reviewed = ReviewState::reviewed();
    let changed = ReviewState::changed_since_review(lines(0, 0));
    let progress: ReviewProgress = [(lines(500, 0), &reviewed), (lines(0, 0), &changed)]
        .into_iter()
        .collect();

    assert_eq!(progress.percent(), 99);
    assert_eq!(progress.filled(12), 11);
}

#[test]
fn changes_after_review_never_exceed_the_file() {
    let changed = ReviewState::changed_since_review(lines(40, 40));
    let progress: ReviewProgress = [(lines(4, 0), &changed)].into_iter().collect();

    assert_eq!(progress.percent(), 0);
}

#[test]
fn an_empty_change_has_no_progress() {
    let progress = ReviewProgress::default();

    assert_eq!(progress.percent(), 0);
    assert_eq!(progress.filled(12), 0);
}
