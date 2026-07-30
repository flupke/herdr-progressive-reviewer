use pr_app::review::{MarkResult, ReviewStatus, ReviewTracker};
use pr_core::repository::{PollResult, Repository, Snapshot};
use pr_state::{LoadResult, ReviewStore};
use pr_tests::{JjFixture, JjLayout};

fn snapshot(repository: &Repository) -> Snapshot {
    match repository.poll().unwrap() {
        PollResult::Complete(snapshot) => snapshot,
        PollResult::ChangedDuringPoll => panic!("test repository changed during a poll"),
    }
}

#[test]
fn baselines_follow_content_not_commit_or_path_identity() {
    let jj = JjFixture::new(JjLayout::NonColocated);
    jj.write("reviewed.txt", b"before\n");
    jj.new_change("review");
    jj.write("reviewed.txt", b"after\n");

    let repository = Repository::discover(jj.root()).unwrap();
    let state = tempfile::tempdir().unwrap();
    let direct_store = ReviewStore::open(state.path(), jj.root()).unwrap();
    let tracker = ReviewTracker::new(
        repository.clone(),
        ReviewStore::open(state.path(), jj.root()).unwrap(),
    );

    let original = snapshot(&repository);
    assert_eq!(
        tracker.mark(&original, &original.files[0]).unwrap(),
        MarkResult::Marked
    );
    assert_eq!(
        tracker.status(&original, &original.files[0]).unwrap(),
        ReviewStatus::Reviewed
    );

    let reviewed_change = jj.change_id();
    jj.new_change("different change");
    assert_eq!(
        tracker.mark(&original, &original.files[0]).unwrap(),
        MarkResult::ChangeChanged
    );
    jj.edit(&reviewed_change);

    let old_parent = format!("{reviewed_change}-");
    jj.jj(["new", &old_parent, "-m", "new parent"]);
    jj.write("unrelated.txt", b"new parent\n");
    let new_parent = jj.change_id();
    jj.jj(["rebase", "-r", &reviewed_change, "-d", &new_parent]);
    jj.edit(&reviewed_change);

    let rebased = snapshot(&repository);
    assert_eq!(
        tracker.status(&rebased, &rebased.files[0]).unwrap(),
        ReviewStatus::Reviewed
    );

    jj.write("reviewed.txt", b"changed again\n");
    let edited = snapshot(&repository);
    assert_eq!(
        tracker.status(&edited, &edited.files[0]).unwrap(),
        ReviewStatus::ChangedSinceReview
    );

    jj.write("reviewed.txt", b"after\n");
    jj.rename("reviewed.txt", "renamed.txt");
    let renamed = snapshot(&repository);
    assert_eq!(
        tracker.status(&renamed, &renamed.files[0]).unwrap(),
        ReviewStatus::Unreviewed
    );

    jj.rename("renamed.txt", "reviewed.txt");
    jj.remove("reviewed.txt");
    let deleted = snapshot(&repository);
    assert_eq!(
        tracker.status(&deleted, &deleted.files[0]).unwrap(),
        ReviewStatus::ChangedSinceReview
    );

    jj.write("reviewed.txt", b"after\n");
    let restored = snapshot(&repository);
    let path = restored.files[0].review_path().as_bytes();
    direct_store
        .mark(
            restored.identity.change_id.as_str(),
            path,
            "1111111111111111111111111111111111111111",
        )
        .unwrap();
    assert_eq!(
        tracker.status(&restored, &restored.files[0]).unwrap(),
        ReviewStatus::Unreviewed
    );
    assert_eq!(
        direct_store
            .load(restored.identity.change_id.as_str(), path)
            .unwrap(),
        LoadResult::Unreviewed
    );
}
