use review_repository::diff::{DiffRow, parse_file_diff};
use review_repository::repository::{ChangedFile, FileKind, Repository, Snapshot};
use review_test_support::{JjFixture, JjLayout, complete_repository_snapshot};
use test_case::test_case;

struct LibraryComparison {
    fixture: JjFixture,
    repository: Repository,
}

impl LibraryComparison {
    fn new(layout: JjLayout) -> Self {
        let fixture = JjFixture::new(layout);
        let repository = Repository::discover(fixture.root()).unwrap();
        Self {
            fixture,
            repository,
        }
    }

    fn assert_diff_matches_cli(&self, snapshot: &Snapshot, file: &ChangedFile) {
        let paths = file
            .old_path
            .iter()
            .chain(&file.new_path)
            .map(|path| {
                format!(
                    "root-file:\"{}\"",
                    std::str::from_utf8(path.as_bytes()).unwrap()
                )
            })
            .collect::<Vec<_>>();
        let mut args = vec![
            "--ignore-working-copy",
            "diff",
            "-r",
            snapshot.identity.snapshot_id(),
            "--git",
            "--",
        ];
        args.extend(paths.iter().map(String::as_str));
        let expected = self.fixture.jj(args).stdout;
        let actual = self.repository.diff(snapshot, file).unwrap();
        let content_rows = |bytes: &[u8]| {
            parse_file_diff(bytes, file)
                .into_iter()
                .filter(|row| !matches!(row, DiffRow::FileHeader { .. } | DiffRow::Meta { .. }))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            content_rows(&actual),
            content_rows(&expected),
            "{}",
            file.display_path
        );
        if let Some(path) = file
            .new_path
            .as_ref()
            .filter(|_| matches!(file.new_kind, FileKind::File | FileKind::Conflict))
        {
            let expected = self
                .fixture
                .jj([
                    "--ignore-working-copy",
                    "file",
                    "show",
                    "-r",
                    snapshot.identity.snapshot_id(),
                    "--",
                    std::str::from_utf8(path.as_bytes()).unwrap(),
                ])
                .stdout;
            assert_eq!(
                self.repository
                    .file_at(snapshot.identity.snapshot_id(), path)
                    .unwrap(),
                expected
            );
        }
    }
}

#[test_case(JjLayout::NonColocated; "non_colocated")]
#[test_case(JjLayout::Colocated; "colocated")]
fn library_reads_match_cli_with_configured_context_and_special_file_changes(layout: JjLayout) {
    let comparison = LibraryComparison::new(layout);
    let fixture = &comparison.fixture;
    fixture.jj(["config", "set", "--repo", "diff.git.context", "1"]);
    fixture.write(
        "changed.rs",
        b"one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\n",
    );
    fixture.write("deleted.txt", b"deleted without newline");
    fixture.write("old name.txt", b"rename\nstable\n");
    fixture.write("binary.dat", b"before\0binary");
    fixture.write("directory", b"was a file\n");
    fixture.new_change("base");
    fixture.write(
        "changed.rs",
        b"one\nchanged\nthree\nfour\nfive\nsix\nseven\nchanged too\nnine\n",
    );
    fixture.remove("deleted.txt");
    fixture.rename("old name.txt", "new name.txt");
    fixture.write("new name.txt", b"rename\nstable\nextra");
    fixture.write("binary.dat", b"after\0binary");
    fixture.remove("directory");
    fixture.write("directory/nested.txt", b"now a directory\n");
    fixture.write("added.txt", b"added without newline");
    let snapshot = complete_repository_snapshot(&comparison.repository);
    for file in &snapshot.files {
        comparison.assert_diff_matches_cli(&snapshot, file);
    }
    assert!(
        comparison
            .repository
            .file_at(
                snapshot.identity.snapshot_id(),
                snapshot
                    .files
                    .iter()
                    .find(|file| file.display_path == "directory")
                    .unwrap()
                    .review_path()
            )
            .is_err()
    );
    fixture.write("added.txt", b"written after the library was opened\n");
    let newer = complete_repository_snapshot(&comparison.repository);
    let file = newer
        .files
        .iter()
        .find(|file| file.display_path == "added.txt")
        .unwrap();
    comparison.assert_diff_matches_cli(&newer, file);
}

#[test_case(JjLayout::NonColocated; "non_colocated")]
#[test_case(JjLayout::Colocated; "colocated")]
fn unresolved_conflict_reads_match_cli_marker_style(layout: JjLayout) {
    let comparison = LibraryComparison::new(layout);
    let fixture = &comparison.fixture;
    fixture.jj(["config", "set", "--repo", "ui.conflict-marker-style", "git"]);
    fixture.write("conflict.txt", b"base\n");
    fixture.new_change("left");
    fixture.write("conflict.txt", b"left\n");
    let left = fixture.change_id();
    fixture.jj(["new", "@-", "-m", "right"]);
    fixture.write("conflict.txt", b"right\n");
    let right = fixture.change_id();
    fixture.jj(["rebase", "-r", &left, "-d", &right]);
    fixture.edit(&left);
    let snapshot = complete_repository_snapshot(&comparison.repository);
    let conflict = snapshot
        .files
        .iter()
        .find(|file| file.display_path == "conflict.txt")
        .unwrap();
    assert_eq!(conflict.new_kind, FileKind::Conflict);
    comparison.assert_diff_matches_cli(&snapshot, conflict);
}

#[test_case(JjLayout::NonColocated; "non_colocated")]
#[test_case(JjLayout::Colocated; "colocated")]
fn merge_commit_diffs_use_the_merged_parent_tree(layout: JjLayout) {
    let comparison = LibraryComparison::new(layout);
    let fixture = &comparison.fixture;
    fixture.write("conflict.txt", b"base\n");
    fixture.new_change("left");
    fixture.write("conflict.txt", b"left\n");
    let left = fixture.change_id();
    fixture.jj(["new", "@-", "-m", "right"]);
    fixture.write("conflict.txt", b"right\n");
    let right = fixture.change_id();
    fixture.jj(["new", &left, &right, "-m", "resolve merge"]);
    fixture.write("conflict.txt", b"resolved\n");
    let snapshot = complete_repository_snapshot(&comparison.repository);
    let file = snapshot
        .files
        .iter()
        .find(|file| file.display_path == "conflict.txt")
        .unwrap();
    comparison.assert_diff_matches_cli(&snapshot, file);
    let base = comparison
        .repository
        .base_file_at(&snapshot, file.old_path.as_ref().unwrap())
        .unwrap();
    assert!(String::from_utf8(base).unwrap().contains("conflict"));
}
