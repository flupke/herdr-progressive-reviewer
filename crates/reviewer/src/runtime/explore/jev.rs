//! Optional bounded significance prefilter. Its output can only remove coverage obligations.
use review_explore::{Comparison, CoverageUnit, Significance, SignificanceResult, SourceSide};
use review_repository::diff::{DiffRow, parse_file_diff};
use serde_json::Value;
#[cfg(all(test, feature = "jev-evals"))]
use serde_json::json;
use std::{collections::BTreeMap, io::Read, time::Duration};

pub(super) const RUBRIC: &str = "explore-significance-checklist-v1";
const MODEL: &str = "jev-1.13.0";
#[cfg(all(test, feature = "jev-evals"))]
const MAX_CANDIDATES: usize = 32;
#[cfg(all(test, feature = "jev-evals"))]
const MAX_STATE_BYTES: usize = 10 * 1024;
const MAX_RESPONSE_BYTES: u64 = 64 * 1024;

mod optimized;
mod sections;

fn coordinate(row: &DiffRow) -> Option<(SourceSide, u32)> {
    match row {
        DiffRow::Add { new_line, .. } => Some((SourceSide::New, *new_line)),
        DiffRow::Delete { old_line, .. } => Some((SourceSide::Old, *old_line)),
        _ => None,
    }
}

#[cfg(all(test, feature = "jev-evals"))]
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
    pub(super) fn prepare(comparison: &Comparison) -> Vec<optimized::Prepared> {
        let mut candidates = Vec::new();
        for (file_index, file) in comparison.files.iter().enumerate() {
            if let Some(diff) = comparison.diffs.get(file_index) {
                let path = file.review_path().display();
                let hunks = optimized::hunks(parse_file_diff(diff, file));
                let frozen = comparison.context.get(file_index);
                let context = optimized::headers(
                    frozen.and_then(|file| file.old_content.as_deref()),
                    frozen.and_then(|file| file.new_content.as_deref()),
                );
                candidates.extend(
                    optimized::SourceFile {
                        id: format!("f{file_index}"),
                        file_index,
                        path: &path,
                        language: optimized::language(&path),
                        context,
                        hunks: &hunks,
                    }
                    .prepare(optimized::TOKEN_BUDGET),
                );
            }
        }
        candidates
    }

    #[cfg(all(test, feature = "jev-evals"))]
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

    #[cfg(all(test, feature = "jev-evals"))]
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
            criterion: optimized::CRITERION.into(),
            input_references: self.references.clone(),
            omissions: self.omissions.clone(),
            probabilities,
            confidence,
            error,
        }
    }
}

#[cfg(all(test, feature = "jev-evals"))]
fn context_text(row: &DiffRow) -> Option<&str> {
    match row {
        DiffRow::Context { text, .. } => Some(text.as_str()),
        _ => None,
    }
}

pub(super) fn classify(
    key: &str,
    candidates: Vec<optimized::Prepared>,
    mut record: impl FnMut(SignificanceResult) -> bool,
) -> bool {
    let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    else {
        return false;
    };
    for planned in candidates {
        let candidate = &planned.candidate;
        let result = if planned.oversized {
            candidate.result(
                Significance::Oversized,
                None,
                BTreeMap::new(),
                None,
                Some(format!("Candidate requires {} estimated tokens, exceeding the {} token budget; it remains required", planned.estimated_tokens, optimized::TOKEN_BUDGET)),
            )
        } else {
            classify_one(&client, key, candidate, &planned.body)
        };
        if !record(result) {
            return false;
        }
    }
    true
}

fn classify_one(
    client: &reqwest::blocking::Client,
    key: &str,
    candidate: &Candidate,
    body: &Value,
) -> SignificanceResult {
    match request_json(client, key, body) {
        Ok(value) => apply_policy(parse_response(candidate, &value)),
        Err(error) => candidate.result(
            Significance::Failed,
            None,
            BTreeMap::new(),
            None,
            Some(error),
        ),
    }
}

fn apply_policy(mut result: SignificanceResult) -> SignificanceResult {
    if result.model.as_deref() != Some(MODEL) {
        result.outcome = Significance::Failed;
        result.error = Some("Unexpected provider model; classification remains required".into());
    } else if result.outcome == Significance::Insignificant
        && result
            .probabilities
            .get("insignificant")
            .copied()
            .unwrap_or_default()
            < 0.85
    {
        result.outcome = Significance::Uncertain;
    }
    result
}

#[cfg(all(test, feature = "jev-evals"))]
impl Candidate {
    fn request(&self) -> Value {
        let question = json!({ "type": "choice", "instructions": INSTRUCTIONS,
            "criteria": { "significant": "This changed block warrants its own explanation because it adds an independent consequential review decision.",
                "insignificant": "The changed lines are locally clear and add no independent review decision, even if a nearby change still warrants review.",
                "uncertain": "Context or understanding is insufficient to decide conservatively." } });
        json!({ "model": MODEL, "state": self.state, "questions": { "significance": question } })
    }
}

// The opt-in evaluator uses this same transport and response parser, retaining
// the raw response for token accounting without changing production decisions.
fn request_json(
    client: &reqwest::blocking::Client,
    key: &str,
    body: &Value,
) -> Result<Value, String> {
    let mut response = client
        .post("https://api.typesafe.ai/v1/systemone")
        .bearer_auth(key)
        .json(body)
        .send()
        .map_err(|_| "Provider request failed or timed out".to_owned())?;
    if !response.status().is_success() {
        return Err(format!("Provider HTTP {}", response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Provider response is unavailable or oversized".to_owned())?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("Provider response is unavailable or oversized".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Malformed provider JSON".into())
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
    let Some(answer) = answers
        .get("checklist")
        .or_else(|| answers.get("significance"))
    else {
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

#[cfg(all(test, feature = "jev-evals"))]
mod evals;
