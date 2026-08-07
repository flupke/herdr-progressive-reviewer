use std::fs;

use review_repository::diff::{DiffRow, parse_file_diff};
use review_repository::repository::{ChangeKind, Repository};
use review_test_support::{GitFixture, complete_repository_snapshot};

#[test]
fn snapshots_git_worktrees_without_changing_the_real_index() {
    // This test uses GitFixture directly because it checks the real Git index.
    let git = GitFixture::new();
    fs::write(git.root().join("modified.txt"), b"before\n").unwrap();
    fs::write(git.root().join("deleted.txt"), b"deleted\n").unwrap();
    fs::write(git.root().join("old-name.txt"), b"rename\n").unwrap();
    git.commit_all("files");

    fs::write(git.root().join("added.txt"), b"added\n").unwrap();
    fs::write(git.root().join("modified.txt"), b"after\n").unwrap();
    fs::remove_file(git.root().join("deleted.txt")).unwrap();
    fs::rename(
        git.root().join("old-name.txt"),
        git.root().join("new-name.txt"),
    )
    .unwrap();
    let state = tempfile::tempdir().unwrap();
    let repository = Repository::discover(git.root())
        .unwrap()
        .with_state_root(state.path());

    let snapshot = complete_repository_snapshot(&repository);
    assert_eq!(snapshot.identity.description(), "Git working tree\n");
    assert_eq!(
        snapshot
            .files
            .iter()
            .map(|file| (file.display_path.as_str(), file.change))
            .collect::<Vec<_>>(),
        [
            ("added.txt", ChangeKind::Added),
            ("deleted.txt", ChangeKind::Deleted),
            ("modified.txt", ChangeKind::Modified),
            ("old-name.txt => new-name.txt", ChangeKind::Renamed),
        ]
    );
    assert_eq!(
        snapshot
            .files
            .iter()
            .map(|file| (file.lines_added, file.lines_removed))
            .collect::<Vec<_>>(),
        [(1, 0), (0, 1), (1, 1), (0, 0)]
    );
    let modified = snapshot
        .files
        .iter()
        .find(|file| file.display_path == "modified.txt")
        .unwrap();
    let rows = parse_file_diff(&repository.diff(&snapshot, modified).unwrap(), modified);
    assert!(
        rows.iter()
            .any(|row| matches!(row, DiffRow::Add { text, .. } if text == "+after"))
    );
    assert!(git.git(["diff", "--cached", "--quiet"]).status.success());
}
