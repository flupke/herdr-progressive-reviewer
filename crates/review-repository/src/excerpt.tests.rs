use super::{DiffExcerpt, ExcerptError};
use crate::diff::DiffRow;

fn rows() -> Vec<DiffRow> {
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
            text: " context".to_owned(),
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
            text: " trailing".to_owned(),
        },
    ]
}

#[test]
fn exact_excerpts_preserve_selected_row_counts_and_headers() {
    let headers = "diff --git a/file b/file\n--- a/file\n+++ b/file";
    for (selection, expected_hunk) in [
        (5..=5, "@@ -10,1 +20,1 @@\n context"),
        (6..=6, "@@ -11,1 +20,0 @@\n-old"),
        (7..=8, "@@ -11,0 +21,2 @@\n+new\n+extra"),
        (6..=8, "@@ -11,1 +21,2 @@\n-old\n+new\n+extra"),
    ] {
        let excerpt = DiffExcerpt::build(&rows(), selection).unwrap();
        assert_eq!(excerpt.as_str(), format!("{headers}\n{expected_hunk}"));
        assert_eq!(excerpt.clone().into_string(), excerpt.as_str());
    }
}

#[test]
fn newline_markers_follow_their_selected_content_row() {
    let mut rows = rows();
    rows.insert(
        8,
        DiffRow::Meta {
            text: "\\ No newline at end of file".to_owned(),
        },
    );

    let excerpt = DiffExcerpt::build(&rows, 7..=7).unwrap();

    assert!(
        excerpt
            .as_str()
            .ends_with("+new\n\\ No newline at end of file")
    );
}

#[test]
fn excerpts_require_complete_headers_selected_content_and_no_notices() {
    assert_eq!(
        DiffExcerpt::build(&rows()[1..], 5..=5),
        Err(ExcerptError::MissingHeaders)
    );
    assert_eq!(
        DiffExcerpt::build(&rows(), 0..=4),
        Err(ExcerptError::NoContent)
    );
    let mut with_notice = rows();
    with_notice.push(DiffRow::Notice {
        kind: crate::diff::NoticeKind::Unsupported,
        text: "unsupported".to_owned(),
    });
    assert_eq!(
        DiffExcerpt::build(&with_notice, 5..=5),
        Err(ExcerptError::Notice)
    );
}
