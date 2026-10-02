use super::*;

fn row(change: RowChange, base: Option<u32>, current: Option<u32>, text: &str) -> OpenRow {
    OpenRow {
        change,
        base_line: base,
        current_line: current,
        text: text.into(),
    }
}

fn diff(name: &str, hunks: &[Vec<OpenRow>]) -> String {
    FileDiff { name, hunks }.to_string()
}

fn diffs() -> UnreviewedDiffs {
    UnreviewedDiffs {
        directory: tempfile::tempdir().unwrap(),
        displaced: Vec::new(),
    }
}

fn read(diffs: &UnreviewedDiffs, name: &str) -> String {
    std::fs::read_to_string(diffs.directory().join(name)).unwrap()
}

#[test]
fn rows_show_the_line_numbers_marks_and_citations_use() {
    let hunks = vec![vec![
        row(RowChange::Unchanged, Some(98), Some(100), "fn keep() {"),
        row(RowChange::Removed, Some(99), None, "    old();"),
        row(
            RowChange::Removed,
            None,
            None,
            "    reviewed_then_rewritten();",
        ),
        row(RowChange::Added, None, Some(101), "    new();"),
    ]];

    assert_eq!(
        diff("src/lib.rs", &hunks),
        concat!(
            "src/lib.rs: unreviewed lines\n",
            "old = line in the base, new = line in the current file. A - row without a number\n",
            "rewrites a reviewed line: mark it with the numbered rows of its change, or the\n",
            "whole file when its change has none.\n",
            "\n",
            "old new\n",
            " 98 100   fn keep() {\n",
            " 99     -     old();\n",
            "        -     reviewed_then_rewritten();\n",
            "    101 +     new();\n",
        )
    );
}

#[test]
fn a_change_without_text_hunks_says_so() {
    assert_eq!(
        diff("logo.png", &[]),
        "logo.png: unreviewed lines\nNo text lines to show: a binary, mode or whole-file change.\n"
    );
}

#[test]
fn a_diff_is_named_like_its_source_file_whatever_its_length() {
    let mut diffs = diffs();
    let long = "x".repeat(255);

    diffs.place("src/deep/a.rs", "nested").unwrap();
    diffs.place(&long, "long").unwrap();
    diffs.write_index().unwrap();

    assert_eq!(read(&diffs, "src/deep/a.rs"), "nested");
    assert_eq!(read(&diffs, &long), "long");
    assert_eq!(diffs.index(), None);
    assert!(!diffs.directory().join(INDEX).exists());
    for outside in ["../escape", "/etc/passwd", "a/../../b"] {
        assert!(diffs.place(outside, "no").is_err(), "{outside}");
    }
}

#[test]
fn a_path_that_is_a_file_and_a_directory_displaces_one_diff_and_the_index_says_where() {
    let mut diffs = diffs();

    // `docs` was a file and is now a directory, in either order.
    diffs.place("docs", "the deleted file").unwrap();
    diffs.place("docs/guide.md", "the new file").unwrap();
    diffs
        .place("src/lib.rs/old.rs", "inside the former directory")
        .unwrap();
    diffs
        .place("src/lib.rs", "the new file of that name")
        .unwrap();
    diffs
        .place("__herdr_reviewer_index__", "a source file of that name")
        .unwrap();
    diffs.write_index().unwrap();

    assert_eq!(read(&diffs, "docs"), "the deleted file");
    assert_eq!(
        read(&diffs, "__herdr_reviewer_displaced_1__"),
        "the new file"
    );
    assert_eq!(
        read(&diffs, "src/lib.rs/old.rs"),
        "inside the former directory"
    );
    assert_eq!(
        read(&diffs, "__herdr_reviewer_displaced_2__"),
        "the new file of that name"
    );
    assert_eq!(diffs.index(), Some(diffs.directory().join(INDEX)));
    assert_eq!(
        read(&diffs, INDEX),
        "These diffs are not at their repository path: another diff uses it as a file or as a \
         directory, or its name is the reviewer's own:\n\
         docs/guide.md -> __herdr_reviewer_displaced_1__\n\
         src/lib.rs -> __herdr_reviewer_displaced_2__\n\
         __herdr_reviewer_index__ -> __herdr_reviewer_displaced_3__\n"
    );
}

#[test]
fn a_write_failure_that_is_not_a_clash_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let mut diffs = diffs();
    // The directory can no longer be written to.
    std::fs::set_permissions(diffs.directory(), std::fs::Permissions::from_mode(0o500)).unwrap();

    let failed = diffs.place("src/lib.rs", "diff");

    std::fs::set_permissions(diffs.directory(), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(failed.is_err());
    assert_eq!(diffs.index(), None);
}
