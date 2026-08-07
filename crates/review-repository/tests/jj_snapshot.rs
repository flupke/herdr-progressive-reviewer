use review_repository::diff::{DiffRow, NoticeKind, parse_file_diff};
use review_repository::repository::{ChangeKind, FileKind, Interdiff, RepoType, Repository};
use review_test_support::{JjFixture, JjLayout, complete_repository_snapshot, repository_fixture};
use test_case::test_case;

struct JjRepositoryTestContext {
    fixture: JjFixture,
    repository: Repository,
}

impl JjRepositoryTestContext {
    fn new(layout: JjLayout) -> Self {
        let fixture = JjFixture::new(layout);
        let repository = Repository::discover(fixture.root()).unwrap();
        assert_eq!(repository.repo_type(), RepoType::Jj);
        Self {
            fixture,
            repository,
        }
    }
}

#[test]
fn discovers_a_git_only_repository() {
    let fixture = repository_fixture(RepoType::Git);

    assert_eq!(
        Repository::discover(fixture.root()).unwrap().repo_type(),
        RepoType::Git
    );
}

#[test]
fn discovers_a_nested_git_repository_before_an_enclosing_jj_repository() {
    // This test needs the Jj root so it can create a nested Git repository.
    let fixture = JjFixture::new(JjLayout::NonColocated);
    let nested = fixture.root().join("nested");
    let output = std::process::Command::new("git")
        .arg("init")
        .arg(&nested)
        .output()
        .unwrap();
    assert!(output.status.success());

    assert_eq!(Repository::discover(&nested).unwrap().root(), nested);
}

#[test_case(JjLayout::NonColocated; "non_colocated")]
#[test_case(JjLayout::Colocated; "colocated")]
fn excludes_change_descriptions_from_file_interdiffs(layout: JjLayout) {
    // This test uses JjFixture directly because it runs Jj-only commands and checks each layout.
    let context = JjRepositoryTestContext::new(layout);
    let fixture = &context.fixture;
    let repository = &context.repository;
    fixture.write("reviewed.txt", b"base\n");
    fixture.write("changed.txt", b"base\n");
    fixture.new_change("initial description");
    fixture.write("reviewed.txt", b"reviewed\n");
    fixture.write("changed.txt", b"first\n");
    let baseline = complete_repository_snapshot(repository);

    fixture.jj([
        "describe",
        "-m",
        "new description\ndiff --git still description",
    ]);
    fixture.write("changed.txt", b"second\n");
    let current = complete_repository_snapshot(repository);
    let interdiff = |path| {
        repository
            .interdiff(baseline.identity.snapshot_id(), &current, path)
            .unwrap()
    };
    let reviewed = current
        .files
        .iter()
        .find(|file| file.display_path == "reviewed.txt")
        .unwrap();
    assert_eq!(
        interdiff(reviewed.review_path()),
        Interdiff::Diff(Vec::new())
    );

    let changed = current
        .files
        .iter()
        .find(|file| file.display_path == "changed.txt")
        .unwrap();
    let Interdiff::Diff(diff) = interdiff(changed.review_path()) else {
        panic!("review snapshot disappeared");
    };
    let diff = String::from_utf8(diff).unwrap();
    assert!(diff.contains("-first"));
    assert!(diff.contains("+second"));
    assert!(!diff.contains("JJ-COMMIT-DESCRIPTION"));
}

#[test_case(JjLayout::NonColocated; "non_colocated")]
#[test_case(JjLayout::Colocated; "colocated")]
fn reports_conflicts_from_real_jj_merges(layout: JjLayout) {
    // This test uses JjFixture directly because it creates a Jj-only merge for each layout.
    let context = JjRepositoryTestContext::new(layout);
    let fixture = &context.fixture;
    let repository = &context.repository;
    fixture.write("conflict.txt", b"base\n");
    fixture.new_change("left");
    fixture.write("conflict.txt", b"left\n");
    fixture.jj(["status"]);
    let left = fixture.change_id();

    fixture.jj(["new", "@-", "-m", "right"]);
    fixture.write("conflict.txt", b"right\n");
    fixture.jj(["status"]);
    let right = fixture.change_id();

    fixture.jj(["rebase", "-r", left.as_str(), "-d", right.as_str()]);
    fixture.edit(&left);
    let snapshot = complete_repository_snapshot(repository);
    let conflict = snapshot
        .files
        .iter()
        .find(|file| file.display_path == "conflict.txt")
        .unwrap();

    assert_eq!(conflict.change, ChangeKind::Conflict);
    assert_eq!(conflict.new_kind, FileKind::Conflict);

    let output = repository.diff(&snapshot, conflict).unwrap();
    let rows = parse_file_diff(&output, conflict);
    assert!(
        rows.iter().any(|row| matches!(
            row,
            DiffRow::Notice {
                kind: NoticeKind::Conflict,
                ..
            }
        )),
        "diff did not contain a conflict notice:\n{}",
        String::from_utf8_lossy(&output)
    );
}
