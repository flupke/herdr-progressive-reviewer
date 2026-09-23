use std::ops::Range;

use review_explore::CoverageUnit;
use review_repository::diff::DiffRow;
use serde_json::{Value, json};

use super::super::Candidate;
pub(super) use super::super::optimized::Boundaries;
use super::dataset::{Case, coordinate};

/// Target ownership is disjoint; context can overlap any part of the original hunk.
pub(super) struct Window<'a> {
    pub(super) case: &'a Case,
    pub(super) rows: &'a [DiffRow],
    pub(super) target: Range<usize>,
    pub(super) context: Range<usize>,
}

impl Window<'_> {
    pub(super) fn candidate(&self) -> Candidate {
        let units: Vec<_> = self.rows[self.target.clone()]
            .iter()
            .filter_map(coordinate)
            .map(|(side, line)| CoverageUnit::Lines {
                file: 0,
                side,
                first: line,
                end: line + 1,
            })
            .collect();
        let references = units.iter().map(|unit| format!("{unit:?}")).collect();
        let omissions = vec!["Other hunks, callers and helpers are not supplied; rows outside this window of the original hunk are omitted.".into()];
        Candidate {
            id: format!("{}-{}-{}", self.case.id, self.target.start, self.target.end),
            units,
            state: json!({
                "path": self.case.path,
                "language_hint": self.case.language,
                "rows": self.rows[self.context.clone()].iter().enumerate().map(|(i, row)| {
                    super::super::optimized::row_json(row, self.target.contains(&(i + self.context.start)))
                }).collect::<Vec<_>>(),
                "omissions": omissions,
            }),
            references,
            omissions,
        }
    }

    pub(super) fn request(candidate: &Candidate) -> Value {
        let mut request = candidate.request();
        let instructions = request["questions"]["significance"]["instructions"]
            .as_str()
            .unwrap();
        request["questions"]["significance"]["instructions"] = format!(
            "{instructions}\nFor this request, the exact changed block is ONLY rows with target=true. Other rows are context and may themselves be added or deleted; their kind preserves that distinction. Judge the target rows together. Choose significant if any target change warrants an independent explanation. Context-only changes must not determine the answer by themselves."
        ).into();
        request
    }
}
