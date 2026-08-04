use std::io::Write as _;
use std::process::{Command, Stdio};

use review_repository::diff::{DiffRow, parse_file_diff};
use review_repository::excerpt::DiffExcerpt;
use review_repository::repository::{PollResult, Repository};
use review_test_support::{JjFixture, JjLayout};

#[test]
fn excerpts_apply_for_each_selection_shape() {
    for layout in [JjLayout::NonColocated, JjLayout::Colocated] {
        let fixture = JjFixture::new(layout);
        fixture.write("sample.txt", base());
        fixture.new_change("edit two distant lines");
        fixture.write("sample.txt", current());

        let repository = Repository::discover(fixture.root()).unwrap();
        let PollResult::Complete(snapshot) = repository.poll().unwrap() else {
            panic!("synchronous fixture poll changed");
        };
        let file = &snapshot.files[0];
        let rows = parse_file_diff(&repository.diff(&snapshot, file).unwrap(), file);
        let deleted: Vec<_> = rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| matches!(row, DiffRow::Delete { .. }).then_some(index))
            .collect();
        let added: Vec<_> = rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| matches!(row, DiffRow::Add { .. }).then_some(index))
            .collect();

        for selection in [
            added[0]..=added[0],
            deleted[0]..=deleted[0],
            deleted[0]..=added[0],
            deleted[0]..=added[1],
        ] {
            let excerpt = DiffExcerpt::build(&rows, selection).unwrap();
            assert!(!excerpt.as_str().ends_with('\n'));
            assert_patch_applies(excerpt.as_str());
        }

        let addition = DiffExcerpt::build(&rows, added[0]..=added[0]).unwrap();
        assert!(!addition.as_str().contains("\n 3"));

        let across_hunks = DiffExcerpt::build(&rows, deleted[0]..=added[1]).unwrap();
        assert_eq!(across_hunks.as_str().matches("@@ -").count(), 2);
    }
}

fn assert_patch_applies(excerpt: &str) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("sample.txt"), base()).unwrap();
    let mut child = Command::new("git")
        .args(["apply", "--check", "--unidiff-zero"])
        .current_dir(directory.path())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{excerpt}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "excerpt must apply:\n{excerpt}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn base() -> &'static [u8] {
    b"one\nold-a\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\nold-b\nfourteen\n"
}

fn current() -> &'static [u8] {
    b"one\nnew-a\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\nnew-b\nfourteen\n"
}
