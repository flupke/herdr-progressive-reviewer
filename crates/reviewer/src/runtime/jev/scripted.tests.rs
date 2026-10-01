use review_explore::SourceSide;
use review_repository::repository::{RepoType, Repository};
use review_test_support::{complete_repository_snapshot, repository_fixture};

use super::*;

fn text(edits: &[(usize, &str)]) -> Vec<u8> {
    let mut lines = (1..=30)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>();
    for (line, replacement) in edits {
        (*replacement).clone_into(&mut lines[*line - 1]);
    }
    lines
        .join("\n")
        .into_bytes()
        .into_iter()
        .chain([b'\n'])
        .collect()
}

/// The results the stand-in records for `script` over a two-hunk change.
fn classify(script: Option<&str>) -> Vec<SignificanceResult> {
    let files = repository_fixture(RepoType::Git);
    files.write("file.rs", &text(&[]));
    files.new_change("review");
    files.write("file.rs", &text(&[(3, "three"), (20, "twenty")]));
    let state = tempfile::tempdir().unwrap();
    let repository = Repository::discover(files.root())
        .unwrap()
        .with_state_root(state.path());
    let comparison =
        Comparison::prepare(&repository, &complete_repository_snapshot(&repository)).unwrap();
    let path = state.path().join("jev-script.json");
    if let Some(script) = script {
        std::fs::write(&path, script).unwrap();
    }
    let mut results = Vec::new();
    assert!(
        ScriptedJev { script: path }
            .plan(&comparison, &|_| false)
            .run(|result| {
                results.push(result);
                true
            })
    );
    results
}

#[test]
fn a_scripted_line_makes_its_whole_hunk_insignificant() {
    let results = classify(Some(
        r#"{"insignificant": [{"path": "file.rs", "lines": [20]}]}"#,
    ));

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "script-f0-h1");
    assert_eq!(results[0].outcome, Significance::Insignificant);
    assert_eq!(
        results[0].units,
        [SourceSide::Old, SourceSide::New].map(|side| ChangeUnit::Lines {
            file: 0,
            side,
            first: 20,
            end: 21,
        })
    );
}

#[test]
fn without_a_script_nothing_is_classified() {
    assert!(classify(None).is_empty());
}

#[test]
fn an_unchanged_line_near_a_hunk_does_not_select_it() {
    assert!(
        classify(Some(
            r#"{"insignificant": [{"path": "file.rs", "lines": [19]}]}"#
        ))
        .is_empty()
    );
}

#[test]
fn a_removed_base_line_selects_its_hunk() {
    let results = classify(Some(
        r#"{"insignificant": [{"path": "file.rs", "lines": [3]}]}"#,
    ));

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "script-f0-h0");
}
