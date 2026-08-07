use review_repository::repository::{RepoType, Repository};
use review_state::{MarkResult, ReviewStatus, ReviewTracker};
use review_store::{LoadResult, ReviewStore};
use review_test_support::{complete_repository_snapshot, repository_fixture};
use test_case::test_case;

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn baselines_follow_content_not_commit_or_path_identity(repository_type: RepoType) {
    let repository_files = repository_fixture(repository_type);
    repository_files.write("reviewed.txt", b"before\n");
    repository_files.new_change("review");
    repository_files.write("reviewed.txt", b"after\n");

    let state = tempfile::tempdir().unwrap();
    let repository = Repository::discover(repository_files.root())
        .unwrap()
        .with_state_root(state.path());
    let direct_store = ReviewStore::open(state.path(), repository_files.root()).unwrap();
    let tracker = ReviewTracker::new(
        repository.clone(),
        ReviewStore::open(state.path(), repository_files.root()).unwrap(),
    );

    let original = complete_repository_snapshot(&repository);
    assert_eq!(
        tracker.mark(&original, &original.files[0]).unwrap(),
        MarkResult::Marked
    );
    assert_eq!(
        tracker
            .status(&original, &original.files[0])
            .unwrap()
            .status,
        ReviewStatus::Reviewed
    );

    let reviewed_change = repository_files.revision_id();
    repository_files.new_change("different change");
    assert_eq!(
        tracker.mark(&original, &original.files[0]).unwrap(),
        MarkResult::ChangeChanged
    );
    repository_files.edit(&reviewed_change);

    repository_files.rewrite_base_without_reviewed_file_change();

    let rebased = complete_repository_snapshot(&repository);
    assert_eq!(
        tracker.status(&rebased, &rebased.files[0]).unwrap().status,
        ReviewStatus::Reviewed
    );

    repository_files.write("reviewed.txt", b"changed again\n");
    let edited = complete_repository_snapshot(&repository);
    assert_eq!(
        tracker.status(&edited, &edited.files[0]).unwrap().status,
        ReviewStatus::ChangedSinceReview
    );

    repository_files.write("reviewed.txt", b"after\n");
    repository_files.rename("reviewed.txt", "renamed.txt");
    let renamed = complete_repository_snapshot(&repository);
    assert_eq!(
        tracker.status(&renamed, &renamed.files[0]).unwrap().status,
        ReviewStatus::Unreviewed
    );

    repository_files.rename("renamed.txt", "reviewed.txt");
    repository_files.remove("reviewed.txt");
    let deleted = complete_repository_snapshot(&repository);
    assert_eq!(
        tracker.status(&deleted, &deleted.files[0]).unwrap().status,
        ReviewStatus::ChangedSinceReview
    );

    repository_files.write("reviewed.txt", b"after\n");
    let restored = complete_repository_snapshot(&repository);
    let path = restored.files[0].review_path().as_bytes();
    direct_store
        .mark(
            restored.identity.review_id(),
            path,
            "1111111111111111111111111111111111111111",
        )
        .unwrap();
    let expired = tracker.status(&restored, &restored.files[0]).unwrap();
    assert_eq!(expired.status, ReviewStatus::Unreviewed);
    assert_eq!(
        expired.warning,
        Some(review_state::ReviewWarning::BaselineExpired)
    );
    assert_eq!(
        direct_store
            .load(restored.identity.review_id(), path)
            .unwrap(),
        LoadResult::Unreviewed
    );
}
