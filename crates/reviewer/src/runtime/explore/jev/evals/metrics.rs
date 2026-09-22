use review_explore::{CoverageUnit, Significance, SignificanceResult};
use serde::Serialize;

use super::dataset::LineLabel;

#[derive(Clone, Default, Serialize)]
pub(super) struct Scores {
    pub(super) significant: u64,
    pub(super) insignificant: u64,
    pub(super) false_exclusions: u64,
    pub(super) correct_exclusions: u64,
    unnecessary_review: u64,
    correct_significant: u64,
    pub(super) uncertain: u64,
    failed: u64,
    oversized: u64,
    pub(super) unassigned: u64,
    probability_lines: u64,
    brier_sum: f64,
}

impl Scores {
    pub(super) fn observe(&mut self, label: &LineLabel, result: Option<&SignificanceResult>) {
        if label.significance == 1 {
            self.significant += 1;
        } else {
            self.insignificant += 1;
        }
        let Some(result) = result else {
            self.unassigned += 1;
            return;
        };
        self.record_outcome(label, &result.outcome);
        self.record_probabilities(label, result);
    }

    fn record_outcome(&mut self, label: &LineLabel, outcome: &Significance) {
        match outcome {
            Significance::Insignificant if label.significance == 1 => self.false_exclusions += 1,
            Significance::Insignificant => self.correct_exclusions += 1,
            Significance::Significant if label.significance == 0 => self.unnecessary_review += 1,
            Significance::Significant => self.correct_significant += 1,
            Significance::Uncertain => self.uncertain += 1,
            Significance::Failed => self.failed += 1,
            Significance::Oversized => self.oversized += 1,
        }
    }

    fn record_probabilities(&mut self, label: &LineLabel, result: &SignificanceResult) {
        if result.probabilities.len() == 3 {
            let expected = if label.significance == 1 {
                "significant"
            } else {
                "insignificant"
            };
            self.brier_sum += result
                .probabilities
                .iter()
                .map(|(key, value)| {
                    let truth = f64::from(key == expected);
                    (value - truth).powi(2)
                })
                .sum::<f64>();
            self.probability_lines += 1;
        }
    }

    pub(super) fn rates(&self) -> serde_json::Value {
        serde_json::json!({
            "false_exclusion_rate": Self::ratio(self.false_exclusions, self.significant),
            "insignificant_exclusion_recall": Self::ratio(self.correct_exclusions, self.insignificant),
            "exclusion_precision": Self::ratio(self.correct_exclusions, self.correct_exclusions + self.false_exclusions),
            "decisive_accuracy": Self::ratio(self.correct_significant + self.correct_exclusions, self.significant + self.insignificant),
            "mean_multiclass_brier": (self.probability_lines > 0).then(|| self.brier_sum / Self::float(self.probability_lines)),
        })
    }

    fn ratio(numerator: u64, denominator: u64) -> Option<f64> {
        (denominator > 0).then(|| Self::float(numerator) / Self::float(denominator))
    }

    #[allow(clippy::cast_precision_loss)]
    fn float(count: u64) -> f64 {
        count as f64
    }
}

pub(super) fn owns(result: &SignificanceResult, unit: &CoverageUnit) -> bool {
    result.units.contains(unit)
}
