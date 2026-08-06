use review_repository::repository::PollResult;
use review_test_support::{JjFixture, JjLayout};

use super::*;

fn snapshot(repository: &Repository) -> Snapshot {
    match repository.poll().unwrap() {
        PollResult::Complete(snapshot) => snapshot,
        PollResult::ChangedDuringPoll => panic!("test repository changed during a poll"),
    }
}

#[test]
fn diff_uses_the_review_baseline() {
    let jj = JjFixture::new(JjLayout::NonColocated);
    jj.write("reviewed.txt", b"before\n");
    jj.new_change("review");
    jj.write("reviewed.txt", b"after\n");
    let repository = Repository::discover(jj.root()).unwrap();
    let state = tempfile::tempdir().unwrap();
    let tracker = ReviewTracker::new(
        repository.clone(),
        ReviewStore::open(state.path(), jj.root()).unwrap(),
    );
    let reviewed = snapshot(&repository);
    let initial = tracker.diff(&reviewed, &reviewed.files[0]).unwrap();
    assert_eq!(initial.old_content.as_deref(), Some(&b"before\n"[..]));
    assert_eq!(initial.new_content.as_deref(), Some(&b"after\n"[..]));
    tracker.mark(&reviewed, &reviewed.files[0]).unwrap();

    jj.write("reviewed.txt", b"after\nfoo\n");
    let changed = snapshot(&repository);
    let loaded = tracker.diff(&changed, &changed.files[0]).unwrap();
    let diff = String::from_utf8(loaded.unified).unwrap();

    assert!(diff.contains("+foo"));
    assert!(!diff.contains("+after"));
    assert_eq!(loaded.old_content.as_deref(), Some(&b"after\n"[..]));
    assert_eq!(loaded.new_content.as_deref(), Some(&b"after\nfoo\n"[..]));
}
