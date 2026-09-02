use review_repository::diff::{DiffRow, NoticeKind, parse_file_diff};
use review_repository::repository::{ChangeKind, FileKind, RepoType, Repository};
use review_test_support::{JjFixture, JjLayout, complete_repository_snapshot};
use test_case::test_case;

#[test_case(JjLayout::NonColocated; "non_colocated")]
#[test_case(JjLayout::Colocated; "colocated")]
fn real_resolved_jj_conflicts_have_no_current_conflict_notice(layout: JjLayout) {
    let fixture = JjFixture::new(layout);
    let repository = Repository::discover(fixture.root()).unwrap();
    assert_eq!(repository.repo_type(), RepoType::Jj);
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
    fixture.write("conflict.txt", b"resolved\n");
    let snapshot = complete_repository_snapshot(&repository);
    let resolved = snapshot
        .files
        .iter()
        .find(|file| file.display_path == "conflict.txt")
        .unwrap();

    assert_eq!(resolved.change, ChangeKind::Modified);
    assert_eq!(resolved.new_kind, FileKind::File);

    let output = repository.diff(&snapshot, resolved).unwrap();
    let rows = parse_file_diff(&output, resolved);
    assert!(!rows.iter().any(|row| matches!(
        row,
        DiffRow::Notice {
            kind: NoticeKind::Conflict,
            ..
        }
    )));
}
