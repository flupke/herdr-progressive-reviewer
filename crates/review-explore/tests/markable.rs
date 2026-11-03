use review_explore::{Comparison, MarkableFiles};
use review_repository::repository::{RepoType, Repository};
use review_test_support::{complete_repository_snapshot, repository_fixture};
use test_case::test_case;

/// A round captured `edited.rs` and `kept.rs`; then `edited.rs` changed again. The marks of
/// the round still apply on `kept.rs`, whose diff is the one the round read, and no longer on
/// `edited.rs`, whose line numbers may name other lines now.
#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn marks_apply_on_the_files_whose_diff_did_not_change(kind: RepoType) {
    let fixture = repository_fixture(kind);
    fixture.write("edited.rs", b"one\ntwo\nthree\n");
    fixture.write("kept.rs", b"alpha\nbeta\n");
    fixture.new_change("base");
    fixture.write("edited.rs", b"one\nTWO\nthree\n");
    fixture.write("kept.rs", b"alpha\nBETA\n");
    let state = tempfile::tempdir().unwrap();
    let repository = Repository::discover(fixture.root())
        .unwrap()
        .with_state_root(state.path());
    let captured = complete_repository_snapshot(&repository);
    let comparison = Comparison::prepare(&repository, &captured).unwrap();
    assert_eq!(
        MarkableFiles::at(&comparison, &repository, &captured),
        MarkableFiles::All
    );

    fixture.write("edited.rs", b"// a comment\none\nTWO\nthree\n");
    let current = complete_repository_snapshot(&repository);

    let markable = MarkableFiles::at(&comparison, &repository, &current);
    let allowed: Vec<_> = current
        .files
        .iter()
        .filter(|file| markable.allows(file))
        .map(|file| file.review_path().display())
        .collect();
    assert_eq!(allowed, ["kept.rs"]);
}
