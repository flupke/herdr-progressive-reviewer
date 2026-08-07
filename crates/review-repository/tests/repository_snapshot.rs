use review_repository::diff::{DiffRow, NoticeKind, parse_file_diff};
use review_repository::repository::{ChangeKind, FileKind, Interdiff, RepoType, Repository};
use review_test_support::{
    ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};
use test_case::test_case;

struct RepositoryTestContext {
    repository_files: Box<dyn ReviewRepositoryFixture>,
    _state_directory: tempfile::TempDir,
    repository: Repository,
}

impl RepositoryTestContext {
    fn new(repository_type: RepoType) -> Self {
        let repository_files = repository_fixture(repository_type);
        let state_directory = tempfile::tempdir().unwrap();
        let repository = Repository::discover(repository_files.root())
            .unwrap()
            .with_state_root(state_directory.path());
        Self {
            repository_files,
            _state_directory: state_directory,
            repository,
        }
    }
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn detects_file_changes(repository_type: RepoType) {
    let context = RepositoryTestContext::new(repository_type);
    context.repository_files.write("modified.txt", b"before\n");
    context.repository_files.write("deleted.txt", b"deleted\n");
    context.repository_files.write("old-name.txt", b"rename\n");
    context.repository_files.new_change("working change");

    context.repository_files.write("added.txt", b"added\n");
    context.repository_files.write("binary.dat", b"\0binary\n");
    context.repository_files.write("modified.txt", b"after\n");
    context.repository_files.remove("deleted.txt");
    context
        .repository_files
        .rename("old-name.txt", "new-name.txt");

    assert_eq!(context.repository.repo_type(), repository_type);
    let snapshot = complete_repository_snapshot(&context.repository);
    let changes: Vec<_> = snapshot
        .files
        .iter()
        .map(|file| (file.display_path.as_str(), file.change))
        .collect();

    assert_eq!(
        changes,
        [
            ("added.txt", ChangeKind::Added),
            ("binary.dat", ChangeKind::Added),
            ("deleted.txt", ChangeKind::Deleted),
            ("modified.txt", ChangeKind::Modified),
            ("old-name.txt => new-name.txt", ChangeKind::Renamed),
        ]
    );
    assert_eq!(snapshot.files[2].new_kind, FileKind::Absent);
    assert!(snapshot.files[2].old_path.is_some());
    assert!(snapshot.files[2].new_path.is_none());
    assert_eq!(
        snapshot
            .files
            .iter()
            .map(|file| (file.lines_added, file.lines_removed))
            .collect::<Vec<_>>(),
        [(1, 0), (0, 0), (0, 1), (1, 1), (0, 0)]
    );

    let deleted_rows = parse_file_diff(
        &context
            .repository
            .diff(&snapshot, &snapshot.files[2])
            .unwrap(),
        &snapshot.files[2],
    );
    assert!(
        deleted_rows
            .iter()
            .any(|row| matches!(row, DiffRow::Delete { old_line: 1, text } if text == "-deleted"))
    );

    let renamed = snapshot.files.last().unwrap();
    let renamed_rows = parse_file_diff(
        &context.repository.diff(&snapshot, renamed).unwrap(),
        renamed,
    );
    assert!(renamed_rows.iter().any(|row| matches!(
        row,
        DiffRow::FileHeader {
            old_path: Some(old_path),
            new_path: Some(new_path),
            ..
        } if Some(old_path) == renamed.old_path.as_ref()
            && Some(new_path) == renamed.new_path.as_ref()
    )));
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn parses_text_and_binary_diffs(repository_type: RepoType) {
    let context = RepositoryTestContext::new(repository_type);
    context
        .repository_files
        .write("text.txt", b"first\nbefore\nlast\n");
    context.repository_files.new_change("working change");
    context
        .repository_files
        .write("text.txt", b"first\nafter\nlast\n");
    context.repository_files.write("binary.dat", b"\0binary\n");

    let snapshot = complete_repository_snapshot(&context.repository);
    let text_file = snapshot
        .files
        .iter()
        .find(|file| file.display_path == "text.txt")
        .unwrap();
    let text_rows = parse_file_diff(
        &context.repository.diff(&snapshot, text_file).unwrap(),
        text_file,
    );

    assert!(
        text_rows
            .iter()
            .any(|row| matches!(row, DiffRow::Delete { old_line: 2, text } if text == "-before"))
    );
    assert!(
        text_rows
            .iter()
            .any(|row| matches!(row, DiffRow::Add { new_line: 2, text } if text == "+after"))
    );
    assert!(text_rows.iter().any(|row| matches!(
        row,
        DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text,
        } if text == " first"
    )));
    assert!(
        text_rows
            .iter()
            .any(|row| matches!(row, DiffRow::Hunk { .. }))
    );
    assert!(!text_rows.iter().any(|row| matches!(
        row,
        DiffRow::Notice {
            kind: NoticeKind::Unsupported,
            ..
        }
    )));

    let binary_file = snapshot
        .files
        .iter()
        .find(|file| file.display_path == "binary.dat")
        .unwrap();
    let binary_rows = parse_file_diff(
        &context.repository.diff(&snapshot, binary_file).unwrap(),
        binary_file,
    );
    assert!(binary_rows.iter().any(|row| matches!(
        row,
        DiffRow::Notice {
            kind: NoticeKind::Binary,
            ..
        }
    )));
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn reports_symbolic_links_without_decoding_their_targets(repository_type: RepoType) {
    let context = RepositoryTestContext::new(repository_type);
    context.repository_files.symlink("target.txt", "link");

    let snapshot = complete_repository_snapshot(&context.repository);
    let link = snapshot
        .files
        .iter()
        .find(|file| file.display_path == "link")
        .unwrap();
    assert_eq!(link.new_kind, FileKind::Symlink);
    let rows = parse_file_diff(&context.repository.diff(&snapshot, link).unwrap(), link);
    assert_eq!(
        rows,
        [DiffRow::Notice {
            kind: NoticeKind::Unsupported,
            text: "Symbolic link target changed; text diff is unavailable".to_owned(),
        }]
    );
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn reports_stable_review_context_switches(repository_type: RepoType) {
    let context = RepositoryTestContext::new(repository_type);
    let first = complete_repository_snapshot(&context.repository);
    assert!(first.files.is_empty());
    let first_revision = context.repository_files.revision_id();

    context.repository_files.write("second.txt", b"second\n");
    context.repository_files.new_change("second change");
    let second = complete_repository_snapshot(&context.repository);
    assert_ne!(second.identity.review_id(), first.identity.review_id());

    context.repository_files.edit(&first_revision);
    let returned = complete_repository_snapshot(&context.repository);
    assert_eq!(returned.identity.review_id(), first.identity.review_id());
    assert_ne!(returned.identity.review_id(), second.identity.review_id());
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn compares_snapshot_trees_after_review(repository_type: RepoType) {
    let context = RepositoryTestContext::new(repository_type);
    context.repository_files.write("reviewed.txt", b"before\n");
    context.repository_files.new_change("file");
    context.repository_files.write("reviewed.txt", b"after\n");
    let reviewed = complete_repository_snapshot(&context.repository);

    context
        .repository_files
        .write("reviewed.txt", b"after\nagain\n");
    let changed = complete_repository_snapshot(&context.repository);
    let Interdiff::Diff(diff) = context
        .repository
        .interdiff(
            reviewed.identity.snapshot_id(),
            &changed,
            changed.files[0].review_path(),
        )
        .unwrap()
    else {
        panic!("review tree disappeared");
    };
    let diff = String::from_utf8(diff).unwrap();
    assert!(diff.contains("+again"));
    assert!(!diff.contains("+after"));
}
