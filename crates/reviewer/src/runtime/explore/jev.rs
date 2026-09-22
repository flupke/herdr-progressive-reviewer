//! Optional bounded significance prefilter. Its output can only remove coverage obligations.
use review_explore::{Comparison, CoverageUnit, Significance, SignificanceResult, SourceSide};
use review_repository::diff::{DiffRow, parse_file_diff};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Read,
    time::{Duration, Instant},
};

pub(super) const RUBRIC: &str = "explore-significance-v2";
const MODEL: &str = "jev-1.13.0";
const MAX_CANDIDATES: usize = 32;
const MAX_STATE_BYTES: usize = 10 * 1024;
const MAX_RESPONSE_BYTES: u64 = 64 * 1024;
const JOB_BUDGET: Duration = Duration::from_secs(30);

const INSTRUCTIONS: &str = "Decide whether THIS exact changed block needs its own explanation in a code review, not whether the surrounding file deserves review. Old changed lines were removed; new changed lines were added. Adjacent context lines are unchanged and only help interpret this block. A change can be insignificant when its local effect is clear but adds no independent review decision: explanatory comments, formatting, routine annotations, or allowing an existing nonessential explanation field to be absent with a default. Significant changes include behavior, policy, state, contracts, dependencies, operations, operational defaults, removed assertions, permissions, and imports with meaningful targets or side effects. A default affecting functional data or compatibility with consequential consumers may still need an explanation. If an unseen consumer, helper, side effect or other context is needed to decide, choose uncertain. Do not infer correctness from a missing source. Answer for this block only.";

#[cfg(not(test))]
pub(super) fn key() -> Option<String> {
    std::env::var("TYPESAFE_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
pub(super) fn key() -> Option<String> {
    None
}

pub(super) struct Candidate {
    id: String,
    units: Vec<CoverageUnit>,
    state: Value,
    references: Vec<String>,
    omissions: Vec<String>,
}

impl Candidate {
    pub(super) fn prepare(comparison: &Comparison) -> Vec<Self> {
        let mut candidates = Vec::new();
        for (file_index, file) in comparison.files.iter().enumerate() {
            if candidates.len() >= MAX_CANDIDATES {
                break;
            }
            if let Some(diff) = comparison.diffs.get(file_index) {
                Self::prepare_file(file_index, file, diff, &mut candidates);
            }
        }
        candidates
    }

    fn prepare_file(
        index: usize,
        file: &review_repository::repository::ChangedFile,
        diff: &[u8],
        candidates: &mut Vec<Self>,
    ) {
        let rows = parse_file_diff(diff, file);
        let mut row = 0;
        let mut block = 0;
        while row < rows.len() && candidates.len() < MAX_CANDIDATES {
            if !matches!(rows[row], DiffRow::Add { .. } | DiffRow::Delete { .. }) {
                row += 1;
                continue;
            }
            let first = row;
            while row < rows.len()
                && matches!(rows[row], DiffRow::Add { .. } | DiffRow::Delete { .. })
            {
                row += 1;
            }
            candidates.push(Self::from_block(index, file, &rows, first, row, block));
            block += 1;
        }
    }

    fn from_block(
        index: usize,
        file: &review_repository::repository::ChangedFile,
        rows: &[DiffRow],
        first: usize,
        end: usize,
        block: usize,
    ) -> Self {
        let mut old = Vec::new();
        let mut new = Vec::new();
        let mut units = Vec::new();
        for changed in &rows[first..end] {
            match changed {
                DiffRow::Delete { old_line, text } => {
                    old.push(text.as_str());
                    units.push(CoverageUnit::Lines {
                        file: index,
                        side: SourceSide::Old,
                        first: *old_line,
                        end: old_line.saturating_add(1),
                    });
                }
                DiffRow::Add { new_line, text } => {
                    new.push(text.as_str());
                    units.push(CoverageUnit::Lines {
                        file: index,
                        side: SourceSide::New,
                        first: *new_line,
                        end: new_line.saturating_add(1),
                    });
                }
                _ => {}
            }
        }
        let before: Vec<_> = rows[..first]
            .iter()
            .rev()
            .take_while(|row| matches!(row, DiffRow::Context { .. }))
            .take(3)
            .filter_map(context_text)
            .collect();
        let after: Vec<_> = rows[end..]
            .iter()
            .take_while(|row| matches!(row, DiffRow::Context { .. }))
            .take(3)
            .filter_map(context_text)
            .collect();
        let path = file.review_path().display().to_string();
        let language_hint = if std::path::Path::new(&path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
        {
            "Rust"
        } else {
            "Unspecified"
        };
        let state = json!({
            "path": path,
            "language_hint": language_hint,
            "unchanged_before": before.into_iter().rev().collect::<Vec<_>>(),
            "removed_lines": old,
            "added_lines": new,
            "unchanged_after": after,
        });
        let references = units
            .iter()
            .filter_map(|unit| match unit {
                CoverageUnit::Lines {
                    side, first, end, ..
                } => Some(format!(
                    "{} {side:?} {first}-{}",
                    file.review_path().display(),
                    end - 1
                )),
                CoverageUnit::Item { .. } => None,
            })
            .collect();
        Self { id: format!("f{index}-b{block}"), units, state, references, omissions: vec!["Only three adjacent unchanged diff rows on each side were included; other callers and helpers were not supplied.".into()] }
    }

    fn result(
        &self,
        outcome: Significance,
        model: Option<String>,
        probabilities: BTreeMap<String, f64>,
        confidence: Option<f64>,
        error: Option<String>,
    ) -> SignificanceResult {
        SignificanceResult {
            id: self.id.clone(),
            units: self.units.clone(),
            outcome,
            model,
            rubric: RUBRIC.into(),
            criterion: INSTRUCTIONS.into(),
            input_references: self.references.clone(),
            omissions: self.omissions.clone(),
            probabilities,
            confidence,
            error,
        }
    }
}

fn context_text(row: &DiffRow) -> Option<&str> {
    match row {
        DiffRow::Context { text, .. } => Some(text.as_str()),
        _ => None,
    }
}

pub(super) fn classify(
    key: &str,
    candidates: Vec<Candidate>,
    mut record: impl FnMut(SignificanceResult) -> bool,
) {
    let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    else {
        return;
    };
    let started = Instant::now();
    for candidate in candidates.into_iter().take(MAX_CANDIDATES) {
        if started.elapsed() >= JOB_BUDGET {
            break;
        }
        let result = if candidate.state.to_string().len() > MAX_STATE_BYTES {
            candidate.result(
                Significance::Oversized,
                None,
                BTreeMap::new(),
                None,
                Some("Candidate exceeds the 10 KiB byte budget; it remains required".into()),
            )
        } else {
            classify_one(&client, key, &candidate)
        };
        if !record(result) {
            break;
        }
    }
}

fn classify_one(
    client: &reqwest::blocking::Client,
    key: &str,
    candidate: &Candidate,
) -> SignificanceResult {
    let question = json!({ "type": "choice", "instructions": INSTRUCTIONS,
        "criteria": { "significant": "This changed block warrants its own explanation because it adds an independent consequential review decision.",
        "insignificant": "The changed lines are locally clear and add no independent review decision, even if a nearby change still warrants review.",
        "uncertain": "Context or understanding is insufficient to decide conservatively." } });
    let body = json!({ "model": MODEL, "state": candidate.state, "questions": { "significance": question } });
    let response = client
        .post("https://api.typesafe.ai/v1/systemone")
        .bearer_auth(key)
        .json(&body)
        .send();
    let Ok(mut response) = response else {
        return candidate.result(
            Significance::Failed,
            None,
            BTreeMap::new(),
            None,
            Some("Provider request failed or timed out".into()),
        );
    };
    if !response.status().is_success() {
        return candidate.result(
            Significance::Failed,
            None,
            BTreeMap::new(),
            None,
            Some(format!("Provider HTTP {}", response.status().as_u16())),
        );
    }
    let mut bytes = Vec::new();
    if response
        .by_ref()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_RESPONSE_BYTES
    {
        return candidate.result(
            Significance::Failed,
            None,
            BTreeMap::new(),
            None,
            Some("Provider response is unavailable or oversized".into()),
        );
    }
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return candidate.result(
            Significance::Failed,
            None,
            BTreeMap::new(),
            None,
            Some("Malformed provider JSON".into()),
        );
    };
    parse_response(candidate, &value)
}

fn parse_response(candidate: &Candidate, value: &Value) -> SignificanceResult {
    let Some(model) = value
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
    else {
        return candidate.result(
            Significance::Failed,
            None,
            BTreeMap::new(),
            None,
            Some("Missing provider model".into()),
        );
    };
    let Some(answers) = value
        .get("answers")
        .and_then(Value::as_object)
        .filter(|answers| answers.len() == 1)
    else {
        return candidate.result(
            Significance::Failed,
            Some(model.into()),
            BTreeMap::new(),
            None,
            Some("Missing or unexpected answer IDs".into()),
        );
    };
    let Some(answer) = answers.get("significance") else {
        return candidate.result(
            Significance::Failed,
            Some(model.into()),
            BTreeMap::new(),
            None,
            Some("Missing significance answer".into()),
        );
    };
    if answer.get("type").and_then(Value::as_str) != Some("choice") {
        return candidate.result(
            Significance::Failed,
            Some(model.into()),
            BTreeMap::new(),
            None,
            Some("Wrong answer type".into()),
        );
    }
    let outcome = match answer.get("choice").and_then(Value::as_str) {
        Some("significant") => Significance::Significant,
        Some("insignificant") => Significance::Insignificant,
        Some("uncertain") => Significance::Uncertain,
        _ => Significance::Failed,
    };
    let Some(probabilities) = answer.get("probabilities").and_then(Value::as_object) else {
        return candidate.result(
            Significance::Failed,
            Some(model.into()),
            BTreeMap::new(),
            None,
            Some("Missing probabilities".into()),
        );
    };
    let Some(parsed) = valid_probabilities(probabilities) else {
        return candidate.result(
            Significance::Failed,
            Some(model.into()),
            BTreeMap::new(),
            None,
            Some("Invalid probabilities".into()),
        );
    };
    let total: f64 = parsed.values().sum();
    let confidence = answer.get("confidence").and_then(Value::as_f64);
    if probabilities.len() != 3
        || !(0.98..=1.02).contains(&total)
        || !confidence.is_some_and(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        || outcome == Significance::Failed
    {
        return candidate.result(
            Significance::Failed,
            Some(model.into()),
            BTreeMap::new(),
            None,
            Some("Invalid provider choice diagnostics".into()),
        );
    }
    candidate.result(outcome, Some(model.into()), parsed, confidence, None)
}

fn valid_probabilities(
    probabilities: &serde_json::Map<String, Value>,
) -> Option<BTreeMap<String, f64>> {
    let mut parsed = BTreeMap::new();
    for option in ["significant", "insignificant", "uncertain"] {
        let value = probabilities.get(option)?.as_f64()?;
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return None;
        }
        parsed.insert(option.into(), value);
    }
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a reviewer-process TYPESAFE_API_KEY and external network"]
    fn live_jev_harmless_fixture() {
        let key = std::env::var("TYPESAFE_API_KEY").expect("reviewer-process key");
        assert!(!key.trim().is_empty());
        let candidate = Candidate {
            id: "harmless".into(),
            units: vec![],
            state: json!({
                "path": "fixture.rs",
                "language_hint": "Rust",
                "unchanged_before": [],
                "removed_lines": ["fn answer() -> i32 { 1 }"],
                "added_lines": ["fn answer() -> i32 { 2 }"],
                "unchanged_after": [],
            }),
            references: vec!["fixture.rs new 1-1".into()],
            omissions: vec![],
        };
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap();
        let result = classify_one(&client, &key, &candidate);
        println!(
            "model={:?} outcome={:?} confidence={:?} error={:?}",
            result.model, result.outcome, result.confidence, result.error
        );
        assert_ne!(
            result.outcome,
            Significance::Failed,
            "provider request failed: {:?}",
            result.error
        );
    }

    #[test]
    #[ignore = "requires a reviewer-process TYPESAFE_API_KEY and external network"]
    fn live_jev_distinguishes_descriptive_and_operational_defaults() {
        let key = std::env::var("TYPESAFE_API_KEY").expect("reviewer-process key");
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap();
        let comment_only = Candidate {
            id: "comment-only".into(),
            units: vec![],
            state: json!({
                "path": "crates/review-explore/src/consequence.rs",
                "language_hint": "Rust",
                "unchanged_before": ["pub struct Consequence {"],
                "removed_lines": [],
                "added_lines": [
                    "    /// Short first paragraph for this Markdown section, including the decisive reason."
                ],
                "unchanged_after": ["    pub summary: String,"]
            }),
            references: vec![],
            omissions: vec![],
        };
        let result = classify_one(&client, &key, &comment_only);
        assert_eq!(result.outcome, Significance::Insignificant, "{result:?}");

        let descriptive = Candidate {
            id: "descriptive-default".into(),
            units: vec![],
            state: json!({
                "path": "crates/review-explore/src/consequence.rs",
                "language_hint": "Rust",
                "unchanged_before": ["pub summary: String,"],
                "removed_lines": [],
                "added_lines": [
                    "    /// Optional Markdown reasoning after the summary; omit or leave empty when unnecessary.",
                    "    #[serde(default)]"
                ],
                "unchanged_after": ["    pub details: String,", "    pub evidence: Vec<EvidenceRef>,"]
            }),
            references: vec![],
            omissions: vec![],
        };
        let result = classify_one(&client, &key, &descriptive);
        assert_eq!(result.outcome, Significance::Insignificant, "{result:?}");

        let operational = Candidate {
            id: "operational-default".into(),
            units: vec![],
            state: json!({
                "path": "src/config.rs",
                "language_hint": "Rust",
                "unchanged_before": ["pub struct RetryConfig {"],
                "removed_lines": ["    #[serde(default = \"one_retry\")]"],
                "added_lines": ["    #[serde(default = \"ten_retries\")]"],
                "unchanged_after": ["    pub max_retries: u32,", "}"]
            }),
            references: vec![],
            omissions: vec![],
        };
        let result = classify_one(&client, &key, &operational);
        assert_eq!(result.outcome, Significance::Significant, "{result:?}");
    }
}
