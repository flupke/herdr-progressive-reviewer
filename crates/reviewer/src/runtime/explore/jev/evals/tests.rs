use std::{collections::BTreeMap, path::Path};

use review_explore::{Significance, SourceSide};
use serde_json::json;

use super::super::{Candidate, apply_policy, optimized, parse_response};
use super::{
    dataset::{Dataset, Fixture, LineLabel},
    metrics::Scores,
    plan::Strategy,
};

fn fixtures() -> Vec<Fixture> {
    Dataset::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/jev-evals"))
        .unwrap()
        .0
}

#[test]
fn labels_cover_both_sides_and_reject_duplicates_or_missing_lines() {
    let mut corpus = fixtures();
    assert!(corpus.len() >= 12);
    let fixture = &mut corpus[0];
    fixture.case.labels.push(fixture.case.labels[0].clone());
    assert!(fixture.validate_labels().is_err());
    fixture.case.labels.pop();
    fixture.case.labels.pop();
    assert!(fixture.validate_labels().is_err());
}

#[test]
fn strategies_own_each_line_once_and_never_leak_labels() {
    for fixture in fixtures() {
        for strategy in Strategy::ALL {
            let chunks = strategy.plan(&fixture, 1024);
            let mut expected: Vec<_> = fixture.case.labels.iter().map(LineLabel::unit).collect();
            let mut actual: Vec<_> = chunks
                .iter()
                .flat_map(|chunk| chunk.candidate.units.clone())
                .collect();
            expected.sort();
            actual.sort();
            assert_eq!(actual, expected, "{} {strategy:?}", fixture.case.id);
            for chunk in &chunks {
                assert!(chunk.oversized || chunk.estimated_tokens <= 1024);
                let state = chunk.body["state"].to_string();
                for label in &fixture.case.labels {
                    assert!(!state.contains(&label.reason), "label leaked into input");
                }
            }
        }
    }
}

#[test]
fn recursion_preserves_targets_and_overlap_adds_context() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.case.id == "large-coverage-hunk")
        .unwrap();
    let plain = Strategy::Recursive.plan(&fixture, 1024);
    let overlap = Strategy::RecursiveOverlap.plan(&fixture, 1024);
    assert!(plain.len() > 1);
    assert_eq!(plain.len(), overlap.len());
    let mut gained_context = false;
    for (plain, overlap) in plain.iter().zip(&overlap) {
        assert_eq!(plain.candidate.units, overlap.candidate.units);
        let before = plain.body["state"]["rows"].as_array().unwrap();
        let after = overlap.body["state"]["rows"].as_array().unwrap();
        assert!(after.len() >= before.len());
        gained_context |= after.len() > before.len();
        for row in before {
            assert!(after.contains(row));
        }
    }
    assert!(gained_context);
    let whole = Strategy::WholeHunk.plan(&fixture, 14_000);
    assert_eq!(whole.len(), 1);
    assert!(
        whole[0].oversized,
        "large fixture must exercise real-budget splitting"
    );
}

#[test]
fn a_fitting_hunk_stays_whole_and_indivisible_changes_stay_required() {
    let corpus = fixtures();
    for strategy in [
        Strategy::WholeHunk,
        Strategy::Recursive,
        Strategy::RecursiveOverlap,
    ] {
        let small = strategy.plan(&corpus[0], 14_000);
        assert_eq!(small.len(), 1);
        assert!(!small[0].oversized);
    }
    let fixture = corpus
        .iter()
        .find(|fixture| fixture.case.id == "indivisible-long-line")
        .unwrap();
    let chunks = Strategy::RecursiveOverlap.plan(fixture, 512);
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].oversized);
    assert_eq!(chunks[0].candidate.units.len(), fixture.case.labels.len());
}

#[test]
fn abstention_is_not_correct_classification_and_false_exclusion_is_counted() {
    let label = LineLabel {
        side: SourceSide::Old,
        line: 1,
        significance: 1,
        category: "guard".into(),
        reason: "policy".into(),
    };
    let candidate = Candidate {
        id: "test".into(),
        units: vec![label.unit()],
        state: json!({}),
        references: vec![],
        omissions: vec![],
    };
    let excluded = candidate.result(
        Significance::Insignificant,
        Some("jev".into()),
        BTreeMap::new(),
        None,
        None,
    );
    let uncertain = candidate.result(
        Significance::Uncertain,
        Some("jev".into()),
        BTreeMap::new(),
        None,
        None,
    );
    let mut scores = Scores::default();
    scores.observe(&label, Some(&excluded));
    scores.observe(&label, Some(&uncertain));
    scores.observe(&label, None);
    assert_eq!(scores.false_exclusions, 1);
    assert_eq!(scores.uncertain, 1);
    assert_eq!(scores.unassigned, 1);
    assert_eq!(scores.rates()["decisive_accuracy"], 0.0);
    let negative = LineLabel {
        significance: 0,
        ..label
    };
    scores.observe(&negative, Some(&excluded));
    assert_eq!(scores.correct_exclusions, 1);
    assert_eq!(scores.rates()["exclusion_precision"], 0.5);
}

#[test]
fn evaluator_reuses_production_parser_and_rejects_bad_diagnostics() {
    let candidate = Candidate {
        id: "test".into(),
        units: vec![],
        state: json!({}),
        references: vec![],
        omissions: vec![],
    };
    let mut raw = json!({"model":"jev-1.13.0","answers":{"significance":{
        "type":"choice","choice":"significant","confidence":0.9,
        "probabilities":{"significant":0.9,"insignificant":0.05,"uncertain":0.05}
    }}});
    assert_eq!(
        parse_response(&candidate, &raw).outcome,
        Significance::Significant
    );
    raw["answers"]["significance"]["probabilities"]["significant"] = json!(5.0);
    assert_eq!(
        parse_response(&candidate, &raw).outcome,
        Significance::Failed
    );
}

#[test]
fn production_winner_preserves_targets_and_requires_high_exclusion_probability() {
    let frozen: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../testdata/jev-evals/study/winner-question.json"
    ))
    .unwrap();
    assert_eq!(optimized::request(&json!({}))["questions"], frozen);
    let profile: super::study::RequestProfile = serde_json::from_value(json!({
        "name":"headers", "prompt":"checklist", "metadata":"headers", "questions":frozen
    }))
    .unwrap();
    assert!(profile.is_production_winner(optimized::TOKEN_BUDGET));
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.case.id == "large-coverage-hunk")
        .unwrap();
    let source = optimized::SourceFile {
        id: fixture.case.id.clone(),
        file_index: 0,
        path: &fixture.case.path,
        language: &fixture.case.language,
        context: fixture.case.context.clone(),
        hunks: &fixture.hunks,
    };
    let chunks = source.prepare(optimized::TOKEN_BUDGET);
    let mut actual: Vec<_> = chunks
        .iter()
        .flat_map(|chunk| chunk.candidate.units.clone())
        .collect();
    let mut expected: Vec<_> = fixture.case.labels.iter().map(LineLabel::unit).collect();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
    assert!(
        chunks
            .iter()
            .all(|chunk| chunk.oversized || chunk.estimated_tokens <= optimized::TOKEN_BUDGET)
    );
    assert!(
        chunks
            .iter()
            .all(|chunk| chunk.body["state"]["file_context"] == fixture.case.context)
    );
    let candidate = &chunks[0].candidate;
    let response = |probability| {
        json!({"model":"jev-1.13.0","answers":{"significance":{
            "type":"choice","choice":"insignificant","confidence":0.9,
            "probabilities":{"significant":1.0-probability,"insignificant":probability,"uncertain":0.0}
        }}})
    };
    assert_eq!(
        apply_policy(parse_response(candidate, &response(0.849))).outcome,
        Significance::Uncertain
    );
    assert_eq!(
        apply_policy(parse_response(candidate, &response(0.85))).outcome,
        Significance::Insignificant
    );
}

#[test]
fn overlap_is_locally_maximal_and_cannot_cross_hunks() {
    use super::{
        dataset::coordinate,
        window::{Boundaries, Window},
    };
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.case.id == "large-coverage-hunk")
        .unwrap();
    let rows = &fixture.hunks[0];
    let boundaries = Boundaries::new(rows);
    for chunk in Strategy::RecursiveOverlap.plan(&fixture, 1024) {
        if chunk.oversized {
            continue;
        }
        let visible = chunk.body["state"]["rows"].as_array().unwrap();
        let locate = |row: &serde_json::Value| {
            rows.iter()
                .position(|source| match source {
                    review_repository::diff::DiffRow::Add { new_line, .. } => {
                        row["kind"] == "added" && row["new"] == *new_line
                    }
                    review_repository::diff::DiffRow::Delete { old_line, .. } => {
                        row["kind"] == "deleted" && row["old"] == *old_line
                    }
                    review_repository::diff::DiffRow::Context { new_line, .. } => {
                        row["kind"] == "unchanged" && row["new"] == *new_line
                    }
                    _ => false,
                })
                .unwrap()
        };
        let start = locate(visible.first().unwrap());
        let end = locate(visible.last().unwrap()) + 1;
        // Target context need not be reconstructed: target=true appears only on
        // changed lines; its first/last changed rows reproduce exactly that set.
        let targets: Vec<_> = visible
            .iter()
            .filter(|row| row["target"] == true)
            .map(locate)
            .collect();
        let target = std::ops::Range {
            start: *targets.first().unwrap(),
            end: targets.last().unwrap() + 1,
        };
        let extend = |context| {
            let candidate = Window {
                case: &fixture.case,
                rows,
                target: target.clone(),
                context,
            }
            .candidate();
            tiktoken_rs::o200k_base_singleton()
                .encode_ordinary(&Window::request(&candidate).to_string())
                .len()
        };
        if let Some(before) = boundaries.points.iter().rev().find(|point| **point < start) {
            assert!(extend(*before..end) > 1024);
        }
        if let Some(after) = boundaries.points.iter().find(|point| **point > end) {
            assert!(extend(start..*after) > 1024);
        }
    }
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.case.id == "multiple-hunks")
        .unwrap();
    let chunks = Strategy::RecursiveOverlap.plan(&fixture, 14_000);
    assert_eq!(chunks.len(), fixture.hunks.len());
    for (chunk, rows) in chunks.iter().zip(&fixture.hunks) {
        assert_eq!(
            chunk.body["state"]["rows"].as_array().unwrap().len(),
            rows.len()
        );
        assert_eq!(
            chunk.candidate.units.len(),
            rows.iter().filter_map(coordinate).count()
        );
    }
}

#[test]
fn paired_function_rewrites_split_without_separating_before_and_after() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.case.id == "paired-function-rewrite")
        .unwrap();
    let whole = Strategy::WholeHunk.plan(&fixture, 1024);
    assert!(whole[0].oversized);
    for strategy in [Strategy::Recursive, Strategy::RecursiveOverlap] {
        let chunks = strategy.plan(&fixture, 1024);
        assert!(chunks.len() > 1);
        for chunk in chunks {
            assert!(!chunk.oversized);
            let rows = chunk.body["state"]["rows"].as_array().unwrap();
            let names = |kind| {
                rows.iter()
                    .filter(|row| row["target"] == true && row["kind"] == kind)
                    .filter_map(|row| {
                        row["text"]
                            .as_str()?
                            .get(1..)?
                            .strip_prefix("fn ")?
                            .split('(')
                            .next()
                    })
                    .collect::<Vec<_>>()
            };
            assert!(!names("deleted").is_empty());
            assert_eq!(names("deleted"), names("added"));
        }
    }
}

#[test]
fn added_declarations_split_at_blank_lines() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.case.id == "large-added-run")
        .unwrap();
    assert!(Strategy::WholeHunk.plan(&fixture, 1024)[0].oversized);
    for strategy in [Strategy::Recursive, Strategy::RecursiveOverlap] {
        let chunks = strategy.plan(&fixture, 1024);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| !chunk.oversized));
    }
}
