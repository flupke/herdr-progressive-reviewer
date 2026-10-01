use std::collections::BTreeMap;

use review_repository::repository::ChangedFile;
use review_source::ReviewCheckpoint;

use super::*;

/// Two modified files: `a.rs` replaces line 1 with two lines, `b.rs` adds one line.
fn comparison() -> Comparison {
    Comparison {
        checkpoint: ReviewCheckpoint::new("review", "checkpoint"),
        repository_root: std::env::temp_dir(),
        files: vec![ChangedFile::modified("a.rs"), ChangedFile::modified("b.rs")],
        diffs: vec![
            b"diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1,2 @@\n-old\n+one\n+two\n"
                .to_vec(),
            b"diff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1,0 +2 @@\n+three\n".to_vec(),
        ],
        context: vec![],
        manifest: vec![],
        sources: vec![],
        base: None,
    }
}

fn lines(file: usize, side: SourceSide, first: u32, end: u32) -> ChangeUnit {
    ChangeUnit::Lines {
        file,
        side,
        first,
        end,
    }
}

fn result(id: &str, units: Vec<ChangeUnit>, outcome: Significance) -> SignificanceResult {
    SignificanceResult {
        id: id.into(),
        units,
        outcome,
        model: None,
        rubric: "test".into(),
        criterion: String::new(),
        input_references: vec![],
        omissions: vec![],
        probabilities: BTreeMap::new(),
        confidence: None,
        error: None,
    }
}

#[test]
fn insignificant_lines_are_excluded_and_a_wholly_excluded_file_qualifies() {
    let comparison = comparison();
    let mut ledger = SignificanceLedger::new(&comparison);

    assert!(ledger.record_significance(result(
        "a",
        vec![
            lines(0, SourceSide::Old, 1, 2),
            lines(0, SourceSide::New, 1, 3)
        ],
        Significance::Insignificant,
    )));
    assert!(ledger.record_significance(result(
        "b",
        vec![lines(1, SourceSide::New, 2, 3)],
        Significance::Significant,
    )));

    assert!(ledger.excluded_lines(0).contains(SourceSide::New, 2));
    assert!(ledger.excluded_lines(1).is_empty());
    assert_eq!(ledger.fully_excluded_files(&comparison), [0]);
    assert!(!ledger.changes_more_than_lines(&comparison, 0));
    assert_eq!(ledger.classifications().count(), 2);
}

#[test]
fn a_result_outside_the_change_or_seen_before_is_refused() {
    let comparison = comparison();
    let mut ledger = SignificanceLedger::new(&comparison);
    let inside = result(
        "a",
        vec![lines(0, SourceSide::New, 1, 2)],
        Significance::Insignificant,
    );

    assert!(ledger.record_significance(inside.clone()));
    assert!(!ledger.record_significance(inside));
    assert!(!ledger.record_significance(result(
        "outside",
        vec![lines(0, SourceSide::New, 7, 8)],
        Significance::Insignificant,
    )));
}
