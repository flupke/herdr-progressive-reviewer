use review_repository::repository::PollResult;
use review_test_support::{GitFixture, JjFixture, JjLayout};
use test_case::test_case;

use super::*;

fn snapshot(repository: &Repository) -> Snapshot {
    match repository.poll().unwrap() {
        PollResult::Complete(snapshot) => snapshot,
        PollResult::ChangedDuringPoll => panic!("test repository changed during a poll"),
    }
}

struct ReviewFixture {
    repository_files: Box<dyn ReviewRepositoryFixture>,
    repository: Repository,
    _state_directory: tempfile::TempDir,
    tracker: ReviewTracker,
    reviewed: Snapshot,
}

#[derive(Clone, Copy, Debug, strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
enum RepositoryKind {
    Git,
    Jj,
}

trait ReviewRepositoryFixture {
    fn root(&self) -> &std::path::Path;
    fn write(&self, contents: &[u8]);
    fn remove(&self);
    fn start_review(&self);
}

impl ReviewRepositoryFixture for GitFixture {
    fn root(&self) -> &std::path::Path {
        self.root()
    }

    fn write(&self, contents: &[u8]) {
        std::fs::write(self.root().join("reviewed.txt"), contents).unwrap();
    }

    fn remove(&self) {
        std::fs::remove_file(self.root().join("reviewed.txt")).unwrap();
    }

    fn start_review(&self) {
        self.commit_all("base");
    }
}

impl ReviewRepositoryFixture for JjFixture {
    fn root(&self) -> &std::path::Path {
        self.root()
    }

    fn write(&self, contents: &[u8]) {
        self.write("reviewed.txt", contents);
    }

    fn remove(&self) {
        self.remove("reviewed.txt");
    }

    fn start_review(&self) {
        self.new_change("review");
    }
}

fn review_fixture(repository_kind: RepositoryKind, reviewed_content: &[u8]) -> ReviewFixture {
    let repository_files: Box<dyn ReviewRepositoryFixture> = match repository_kind {
        RepositoryKind::Git => Box::new(GitFixture::new()),
        RepositoryKind::Jj => Box::new(JjFixture::new(JjLayout::NonColocated)),
    };
    repository_files.write(b"before\n");
    repository_files.start_review();
    repository_files.write(reviewed_content);
    let state_directory = tempfile::tempdir().unwrap();
    let repository = Repository::discover(repository_files.root())
        .unwrap()
        .with_state_root(state_directory.path());
    let tracker = ReviewTracker::new(
        repository.clone(),
        ReviewStore::open(state_directory.path(), repository_files.root()).unwrap(),
    );
    let reviewed = snapshot(&repository);
    ReviewFixture {
        repository_files,
        repository,
        _state_directory: state_directory,
        tracker,
        reviewed,
    }
}

#[test_case(RepositoryKind::Git; "git")]
#[test_case(RepositoryKind::Jj; "jj")]
fn diff_uses_the_review_baseline(repository_kind: RepositoryKind) {
    let fixture = review_fixture(repository_kind, b"after\n");
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

    fixture.repository_files.write(b"after\nfoo\n");
    let changed = snapshot(&fixture.repository);
    let loaded = fixture.tracker.diff(&changed, &changed.files[0]).unwrap();
    let diff = String::from_utf8(loaded.unified).unwrap();

    assert!(diff.contains("+foo"));
    assert!(!diff.contains("+after"));
    assert_eq!(loaded.old_content.as_deref(), Some(&b"after\n"[..]));
    assert_eq!(loaded.new_content.as_deref(), Some(&b"after\nfoo\n"[..]));
}

#[test_case(RepositoryKind::Git; "git")]
#[test_case(RepositoryKind::Jj; "jj")]
fn deleted_file_diff_includes_the_reviewed_content(repository_kind: RepositoryKind) {
    let fixture = review_fixture(repository_kind, b"reviewed content\n");
    fixture
        .tracker
        .mark(&fixture.reviewed, &fixture.reviewed.files[0])
        .unwrap();

    fixture.repository_files.remove();
    let deleted = snapshot(&fixture.repository);
    let loaded = fixture.tracker.diff(&deleted, &deleted.files[0]).unwrap();

    assert_eq!(
        loaded.old_content.as_deref(),
        Some(&b"reviewed content\n"[..])
    );
    assert_eq!(loaded.new_content, None);
}

#[test_case(RepositoryKind::Git; "git")]
#[test_case(RepositoryKind::Jj; "jj")]
fn unreview_removes_the_stored_review_mark(repository_kind: RepositoryKind) {
    let fixture = review_fixture(repository_kind, b"after\n");
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
