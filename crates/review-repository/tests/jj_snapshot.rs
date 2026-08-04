use review_repository::diff::{DiffRow, NoticeKind, parse_file_diff};
use review_repository::repository::{
    ChangeKind, FileKind, Interdiff, PollResult, RepoType, Repository, Snapshot,
};
use review_test_support::{JjFixture, JjLayout};

fn complete_snapshot(repository: &Repository) -> Snapshot {
    match repository.poll().unwrap() {
        PollResult::Complete(snapshot) => snapshot,
        PollResult::ChangedDuringPoll => panic!("fixture changed during a synchronous poll"),
    }
}

fn for_each_layout(mut test: impl FnMut(&JjFixture, &Repository)) {
    for layout in [JjLayout::NonColocated, JjLayout::Colocated] {
        let fixture = JjFixture::new(layout);
        let repository = Repository::discover(fixture.root()).unwrap();
        assert_eq!(repository.repo_type(), RepoType::Jj);
        test(&fixture, &repository);
    }
}

#[test]
fn detects_file_changes_in_real_jj_repositories() {
    for_each_layout(|fixture, repository| {
        fixture.write("modified.txt", b"before\n");
        fixture.write("deleted.txt", b"deleted\n");
        fixture.write("old-name.txt", b"rename\n");
        fixture.new_change("working change");

        fixture.write("added.txt", b"added\n");
        fixture.write("binary.dat", b"\0binary\n");
        fixture.write("modified.txt", b"after\n");
        fixture.remove("deleted.txt");
        fixture.rename("old-name.txt", "new-name.txt");

        let snapshot = complete_snapshot(repository);
        assert_eq!(snapshot.identity.description(), "working change\n");
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
            &repository.diff(&snapshot, &snapshot.files[2]).unwrap(),
            &snapshot.files[2],
        );
        assert!(
            deleted_rows.iter().any(
                |row| matches!(row, DiffRow::Delete { old_line: 1, text } if text == "-deleted")
            )
        );

        let renamed = snapshot.files.last().unwrap();
        let renamed_rows = parse_file_diff(&repository.diff(&snapshot, renamed).unwrap(), renamed);
        assert!(renamed_rows.iter().any(|row| matches!(
            row,
            DiffRow::FileHeader {
                old_path: Some(old_path),
                new_path: Some(new_path),
                ..
            } if Some(old_path) == renamed.old_path.as_ref()
                && Some(new_path) == renamed.new_path.as_ref()
        )));
    });
}

#[test]
fn discovers_a_git_only_repository() {
    let directory = tempfile::tempdir().unwrap();
    let output = std::process::Command::new("git")
        .arg("init")
        .arg(directory.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    assert_eq!(
        Repository::discover(directory.path()).unwrap().repo_type(),
        RepoType::Git
    );
}

#[test]
fn discovers_a_nested_git_repository_before_an_enclosing_jj_repository() {
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

#[test]
fn parses_text_and_binary_diffs_from_real_jj_repositories() {
    for_each_layout(|fixture, repository| {
        fixture.write("text.txt", b"first\nbefore\nlast\n");
        fixture.new_change("working change");
        fixture.write("text.txt", b"first\nafter\nlast\n");
        fixture.write("binary.dat", b"\0binary\n");

        let snapshot = complete_snapshot(repository);
        let text_file = snapshot
            .files
            .iter()
            .find(|file| file.display_path == "text.txt")
            .unwrap();
        let text_rows = parse_file_diff(&repository.diff(&snapshot, text_file).unwrap(), text_file);

        assert!(
            text_rows.iter().any(
                |row| matches!(row, DiffRow::Delete { old_line: 2, text } if text == "-before")
            )
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
            &repository.diff(&snapshot, binary_file).unwrap(),
            binary_file,
        );
        assert!(binary_rows.iter().any(|row| matches!(
            row,
            DiffRow::Notice {
                kind: NoticeKind::Binary,
                ..
            }
        )));
    });
}

#[test]
fn reports_symbolic_links_without_decoding_their_targets() {
    for_each_layout(|fixture, repository| {
        fixture.symlink("target.txt", "link");

        let snapshot = complete_snapshot(repository);
        let link = snapshot
            .files
            .iter()
            .find(|file| file.display_path == "link")
            .unwrap();
        assert_eq!(link.new_kind, FileKind::Symlink);
        let rows = parse_file_diff(&repository.diff(&snapshot, link).unwrap(), link);
        assert_eq!(
            rows,
            [DiffRow::Notice {
                kind: NoticeKind::Unsupported,
                text: "Symbolic link target changed; text diff is unavailable".to_owned(),
            }]
        );
    });
}

#[test]
fn reports_empty_changes_and_stable_change_switches() {
    for_each_layout(|fixture, repository| {
        let first = complete_snapshot(repository);
        assert!(first.files.is_empty());
        let first_change = fixture.change_id();

        fixture.new_change("second change");
        fixture.write("second.txt", b"second\n");
        let second = complete_snapshot(repository);
        assert_ne!(second.identity.review_id(), first.identity.review_id());

        fixture.edit(&first_change);
        let returned = complete_snapshot(repository);
        assert_eq!(returned.identity.review_id(), first.identity.review_id());
        assert_ne!(returned.identity.review_id(), second.identity.review_id());
    });
}

#[test]
fn excludes_change_descriptions_from_file_interdiffs() {
    for_each_layout(|fixture, repository| {
        fixture.write("reviewed.txt", b"base\n");
        fixture.write("changed.txt", b"base\n");
        fixture.new_change("initial description");
        fixture.write("reviewed.txt", b"reviewed\n");
        fixture.write("changed.txt", b"first\n");
        let baseline = complete_snapshot(repository);

        fixture.jj([
            "describe",
            "-m",
            "new description\ndiff --git still description",
        ]);
        fixture.write("changed.txt", b"second\n");
        let current = complete_snapshot(repository);
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
    });
}

#[test]
fn reports_conflicts_from_real_jj_merges() {
    for_each_layout(|fixture, repository| {
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
        let snapshot = complete_snapshot(repository);
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
    });
}
