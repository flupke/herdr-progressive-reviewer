use review_repository::repository::RepoType;
use review_test_support::{
    ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};
use test_case::test_case;

use super::*;

struct ReviewFixture {
    repository_files: Box<dyn ReviewRepositoryFixture>,
    repository: Repository,
    _state_directory: tempfile::TempDir,
    tracker: ReviewTracker,
    reviewed: Snapshot,
}

fn review_fixture(repository_type: RepoType, reviewed_content: &[u8]) -> ReviewFixture {
    let repository_files = repository_fixture(repository_type);
    repository_files.write("reviewed.txt", b"before\n");
    repository_files.new_change("review");
    repository_files.write("reviewed.txt", reviewed_content);
    let state_directory = tempfile::tempdir().unwrap();
    let repository = Repository::discover(repository_files.root())
        .unwrap()
        .with_state_root(state_directory.path());
    let tracker = ReviewTracker::new(
        repository.clone(),
        ReviewStore::open(state_directory.path(), repository_files.root()).unwrap(),
    );
    let reviewed = complete_repository_snapshot(&repository);
    ReviewFixture {
        repository_files,
        repository,
        _state_directory: state_directory,
        tracker,
        reviewed,
    }
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn diff_uses_the_review_baseline(repository_type: RepoType) {
    let fixture = review_fixture(repository_type, b"after\n");
    let initial = fixture
        .tracker
        .diff(&fixture.reviewed, &fixture.reviewed.files[0])
        .unwrap();
    assert_eq!(initial.old_content.as_deref(), Some(&b"before\n"[..]));
    assert_eq!(initial.new_content.as_deref(), Some(&b"after\n"[..]));
    fixture
        .tracker
        .mark(&fixture.reviewed, &fixture.reviewed.files[0])
        .unwrap();

    fixture
        .repository_files
        .write("reviewed.txt", b"after\nfoo\n");
    let changed = complete_repository_snapshot(&fixture.repository);
    let loaded = fixture.tracker.diff(&changed, &changed.files[0]).unwrap();
    let diff = String::from_utf8(loaded.unified).unwrap();

    assert!(diff.contains("+foo"));
    assert!(!diff.contains("+after"));
    assert_eq!(loaded.old_content.as_deref(), Some(&b"after\n"[..]));
    assert_eq!(loaded.new_content.as_deref(), Some(&b"after\nfoo\n"[..]));
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn deleted_file_diff_includes_the_reviewed_content(repository_type: RepoType) {
    let fixture = review_fixture(repository_type, b"reviewed content\n");
    fixture
        .tracker
        .mark(&fixture.reviewed, &fixture.reviewed.files[0])
        .unwrap();

    fixture.repository_files.remove("reviewed.txt");
    let deleted = complete_repository_snapshot(&fixture.repository);
    let loaded = fixture.tracker.diff(&deleted, &deleted.files[0]).unwrap();

    assert_eq!(
        loaded.old_content.as_deref(),
        Some(&b"reviewed content\n"[..])
    );
    assert_eq!(loaded.new_content, None);
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn unreview_removes_the_stored_review_mark(repository_type: RepoType) {
    let fixture = review_fixture(repository_type, b"after\n");
    fixture
        .tracker
        .mark(&fixture.reviewed, &fixture.reviewed.files[0])
        .unwrap();

    fixture
        .tracker
        .unreview(&fixture.reviewed, &fixture.reviewed.files[0])
        .unwrap();

    assert_eq!(
        fixture
            .tracker
            .status(&fixture.reviewed, &fixture.reviewed.files[0])
            .unwrap()
            .status,
        ReviewStatus::Unreviewed
    );
}
