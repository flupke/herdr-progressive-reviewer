use std::fs;
use std::path::PathBuf;

use review_test_support::{GitFixture, JjFixture, JjLayout};
use review_types::ReviewUnit;

use crate::repository::Repository;

fn canonical(path: impl Into<PathBuf>) -> PathBuf {
    fs::canonicalize(path.into()).unwrap()
}

#[test]
fn every_jj_workspace_shares_the_main_workspace_store() {
    let fixture = JjFixture::new(JjLayout::NonColocated);
    let second = fixture.root().with_file_name("second");
    fixture.jj(["workspace", "add", second.to_str().unwrap()]);

    let main = Repository::discover(fixture.root()).unwrap();
    let task = Repository::discover(&second).unwrap();

    let store = canonical(fixture.root().join(".jj/repo"));
    assert_eq!(main.shared_store().unwrap(), store);
    assert_eq!(task.shared_store().unwrap(), store);
    let mut roots = task.workspace_roots().unwrap();
    roots.sort();
    let mut expected = vec![canonical(fixture.root()), canonical(&second)];
    expected.sort();
    assert_eq!(roots, expected);
}

#[test]
fn a_jj_repository_holds_its_hidden_changes_but_not_the_root_or_foreign_ones() {
    let fixture = JjFixture::new(JjLayout::Colocated);
    fixture.write("a.txt", b"a\n");
    fixture.new_change("kept");
    let kept: ReviewUnit = fixture.change_id().into();
    fixture.new_change("abandoned");
    fixture.write("b.txt", b"b\n");
    let abandoned: ReviewUnit = fixture.change_id().into();
    fixture.jj(["abandon", "@"]);
    let repository = Repository::discover(fixture.root()).unwrap();
    let root: ReviewUnit = "z".repeat(32).into();
    let foreign: ReviewUnit = "k".repeat(32).into();
    let tree: ReviewUnit = "4b825dc642cb6eb9a060e54bf8d69288fbee4904".into();

    assert!(repository.holds_any_review_unit([&kept]).unwrap());
    assert!(repository.holds_any_review_unit([&abandoned]).unwrap());
    assert!(
        !repository
            .holds_any_review_unit([&root, &foreign, &tree])
            .unwrap()
    );
    assert!(
        repository
            .holds_any_review_unit([&foreign, &abandoned])
            .unwrap()
    );
}

#[test]
fn every_git_worktree_shares_the_common_directory_and_its_trees() {
    let fixture = GitFixture::new();
    let second = fixture.root().with_file_name("second");
    fixture.git(["worktree", "add", "--quiet", second.to_str().unwrap()]);
    let task = Repository::discover(&second).unwrap();
    let tree = String::from_utf8(fixture.git(["rev-parse", "HEAD^{tree}"]).stdout).unwrap();
    let tree: ReviewUnit = tree.trim().into();
    let foreign: ReviewUnit = "0".repeat(40).into();
    let change: ReviewUnit = "k".repeat(32).into();

    assert_eq!(
        task.shared_store().unwrap(),
        canonical(fixture.root().join(".git"))
    );
    let mut roots = task.workspace_roots().unwrap();
    roots.sort();
    let mut expected = vec![canonical(fixture.root()), canonical(&second)];
    expected.sort();
    assert_eq!(roots, expected);
    assert!(task.holds_any_review_unit([&foreign, &tree]).unwrap());
    assert!(!task.holds_any_review_unit([&foreign, &change]).unwrap());
}
