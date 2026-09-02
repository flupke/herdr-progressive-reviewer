use super::{DiffParser, DiffRow, MAX_LINE_BYTES, NoticeKind, parse_file_diff};
use crate::repository::{ChangeKind, ChangedFile, FileKind, RepoPath};

#[test]
fn rejects_a_line_above_the_parse_limit() {
    assert_eq!(MAX_LINE_BYTES, 16_777_216);
    let rows = DiffParser::parse(&vec![b'x'; MAX_LINE_BYTES + 1]);

    assert!(matches!(
        rows.as_slice(),
        [DiffRow::Notice {
            kind: NoticeKind::Unsupported,
            ..
        }]
    ));
}

#[test]
fn parses_and_numbers_each_unified_diff_row() {
    let rows = DiffParser::parse(
        b"diff --git a/file b/file\nindex 111..222 100644\n--- a/file\n+++ b/file\n@@ -10,3 +20,4 @@\n context one\n-old\n+new\n+extra\n context two\n",
    );

    assert_eq!(
        rows,
        vec![
            DiffRow::FileHeader {
                old_path: None,
                new_path: None,
                text: "diff --git a/file b/file".to_owned(),
            },
            DiffRow::Meta {
                text: "index 111..222 100644".to_owned(),
            },
            DiffRow::Meta {
                text: "--- a/file".to_owned(),
            },
            DiffRow::Meta {
                text: "+++ b/file".to_owned(),
            },
            DiffRow::Hunk {
                old_start: 10,
                old_count: 3,
                new_start: 20,
                new_count: 4,
            },
            DiffRow::Context {
                old_line: 10,
                new_line: 20,
                text: " context one".to_owned(),
            },
            DiffRow::Delete {
                old_line: 11,
                text: "-old".to_owned(),
            },
            DiffRow::Add {
                new_line: 21,
                text: "+new".to_owned(),
            },
            DiffRow::Add {
                new_line: 22,
                text: "+extra".to_owned(),
            },
            DiffRow::Context {
                old_line: 12,
                new_line: 23,
                text: " context two".to_owned(),
            },
        ]
    );
}

#[test]
fn incomplete_hunks_are_reported_at_the_end_and_before_a_new_file() {
    for suffix in ["", "diff --git a/next b/next\n"] {
        let diff = format!("@@ -1,2 +1,2 @@\n-old\n+new\n{suffix}");
        let rows = DiffParser::parse(diff.as_bytes());
        assert!(rows.iter().any(|row| matches!(
            row,
            DiffRow::Notice {
                kind: NoticeKind::Unsupported,
                text,
            } if text == "Unified diff hunk row count does not match its header"
        )));
    }
}

#[test]
fn each_conflict_marker_is_reported_and_advances_its_side() {
    for marker in [
        " <<<<<<< conflict 1 of 1",
        " %%%%%%% diff from: base",
        r" \\\\\\\        to: side",
        " >>>>>>> conflict 1 of 1 ends",
    ] {
        let diff = format!("@@ -7,1 +9,1 @@\n{marker}\n");
        let rows = DiffParser::parse(diff.as_bytes());
        assert!(
            matches!(
                rows.as_slice(),
                [
                    DiffRow::Hunk { .. },
                    DiffRow::Notice {
                        kind: NoticeKind::Conflict,
                        ..
                    }
                ]
            ),
            "{marker:?}: {rows:?}"
        );
    }
}

fn changed_file(old_kind: FileKind, new_kind: FileKind) -> ChangedFile {
    ChangedFile {
        old_path: Some(RepoPath::from_bytes(b"file")),
        new_path: Some(RepoPath::from_bytes(b"file")),
        old_kind,
        new_kind,
        change: ChangeKind::Modified,
        display_path: "file".to_owned(),
        lines_added: 0,
        lines_removed: 0,
    }
}

#[test]
fn special_file_kinds_do_not_produce_text_diffs_on_either_side() {
    for (kind, expected) in [
        (FileKind::Symlink, "Symbolic link target changed"),
        (FileKind::Gitlink, "Git submodule changed"),
    ] {
        for file in [
            changed_file(kind, FileKind::File),
            changed_file(FileKind::File, kind),
        ] {
            let rows = parse_file_diff(b"text that must not be parsed", &file);
            assert!(matches!(
                rows.as_slice(),
                [DiffRow::Notice {
                    kind: NoticeKind::Unsupported,
                    text,
                }] if text.starts_with(expected)
            ));
        }
    }
}

#[test]
fn conflict_kind_adds_one_notice_only_when_markers_are_absent() {
    let file = changed_file(FileKind::File, FileKind::Conflict);
    let without_marker = parse_file_diff(b"@@ -1,1 +1,1 @@\n-old\n+new\n", &file);
    assert_eq!(
        without_marker
            .iter()
            .filter(|row| matches!(
                row,
                DiffRow::Notice {
                    kind: NoticeKind::Conflict,
                    ..
                }
            ))
            .count(),
        1
    );
    let with_marker = parse_file_diff(b"@@ -1,1 +1,1 @@\n <<<<<<< conflict\n", &file);
    assert_eq!(
        with_marker
            .iter()
            .filter(|row| matches!(
                row,
                DiffRow::Notice {
                    kind: NoticeKind::Conflict,
                    ..
                }
            ))
            .count(),
        1
    );
}

#[test]
fn resolved_conflict_markers_are_normal_diff_rows() {
    let file = changed_file(FileKind::Conflict, FileKind::File);
    let rows = parse_file_diff(
        b"@@ -1,2 +1,1 @@\n-<<<<<<< conflict 1 of 1\n-old\n+resolved\n",
        &file,
    );

    assert!(matches!(rows[1], DiffRow::Delete { old_line: 1, .. }));
    assert!(matches!(rows[2], DiffRow::Delete { old_line: 2, .. }));
    assert!(matches!(rows[3], DiffRow::Add { new_line: 1, .. }));
    assert!(!rows.iter().any(|row| matches!(
        row,
        DiffRow::Notice {
            kind: NoticeKind::Conflict,
            ..
        }
    )));
}

#[test]
fn reports_unknown_lines_outside_hunks_as_unsupported() {
    let rows = DiffParser::parse(b"unexpected diff output\n");

    assert!(matches!(
        rows.as_slice(),
        [DiffRow::Notice {
            kind: NoticeKind::Unsupported,
            text,
        }] if text == "unexpected diff output"
    ));
}
