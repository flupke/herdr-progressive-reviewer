//! A stand-in for Jev in vision sessions: a script names the hunks to call
//! insignificant, so an exploration can drive `rf` without the paid service.

use std::path::PathBuf;

use review_explore::Comparison;

use review_repository::diff::{DiffRow, parse_file_diff};
use review_significance::{
    ChangeUnit, Significance, SignificanceClassifier, SignificancePlan, SignificanceResult,
};
use serde_json::Value;

use super::{coordinate, optimized};

const RUBRIC: &str = "vision script";

/// Classifies as insignificant every diff hunk that changes a scripted line:
/// a line it adds (current numbering) or a line it removes (base
/// numbering). The script is JSON read when `rf` starts:
/// `{"insignificant": [{"path": "src/lib.rs", "lines": [20]}]}`.
pub(super) struct ScriptedJev {
    pub(super) script: PathBuf,
}

impl SignificanceClassifier for ScriptedJev {
    fn rubric(&self) -> &str {
        RUBRIC
    }

    fn plan(&self, comparison: &Comparison, classified: &dyn Fn(&str) -> bool) -> SignificancePlan {
        let results = self
            .results(comparison)
            .into_iter()
            .filter(|result| !classified(&result.id))
            .collect::<Vec<_>>();
        SignificancePlan::new(results.len(), move |record| results.into_iter().all(record))
    }
}

impl ScriptedJev {
    fn results(&self, comparison: &Comparison) -> Vec<SignificanceResult> {
        let script = std::fs::read(&self.script)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or_default();
        let requests = script["insignificant"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        comparison
            .files
            .iter()
            .zip(&comparison.diffs)
            .enumerate()
            .flat_map(|(index, (file, diff))| {
                let path = file.review_path().display();
                let lines = requests
                    .iter()
                    .filter(|request| request["path"].as_str() == Some(path.as_str()))
                    .flat_map(|request| request["lines"].as_array().cloned().unwrap_or_default())
                    .filter_map(|line| u32::try_from(line.as_u64()?).ok())
                    .collect::<Vec<_>>();
                optimized::hunks(parse_file_diff(diff, file))
                    .into_iter()
                    .enumerate()
                    .filter_map(move |(hunk, rows)| scripted_hunk(index, hunk, &rows, &lines))
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

/// The insignificant result for one diff hunk, when it changes a scripted line.
fn scripted_hunk(
    file: usize,
    hunk: usize,
    rows: &[DiffRow],
    lines: &[u32],
) -> Option<SignificanceResult> {
    let changed = rows.iter().filter_map(coordinate).collect::<Vec<_>>();
    changed
        .iter()
        .any(|(_, line)| lines.contains(line))
        .then(|| SignificanceResult {
            id: format!("script-f{file}-h{hunk}"),
            units: changed
                .into_iter()
                .map(|(side, line)| ChangeUnit::Lines {
                    file,
                    side,
                    first: line,
                    end: line + 1,
                })
                .collect(),
            outcome: Significance::Insignificant,
            model: None,
            rubric: RUBRIC.into(),
            criterion: String::new(),
            input_references: Vec::new(),
            omissions: Vec::new(),
            probabilities: std::collections::BTreeMap::default(),
            confidence: None,
            error: None,
        })
}

#[cfg(test)]
#[path = "scripted.tests.rs"]
mod tests;
