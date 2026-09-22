use std::ops::Range;

use review_explore::CoverageUnit;
use review_repository::diff::DiffRow;
use serde_json::{Value, json};

use super::super::Candidate;
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
                    Self::row(row, self.target.contains(&(i + self.context.start)))
                }).collect::<Vec<_>>(),
                "omissions": omissions,
            }),
            references,
            omissions,
        }
    }

    fn row(row: &DiffRow, target: bool) -> Value {
        match row {
            DiffRow::Add { new_line, text } => {
                json!({"kind":"added", "new":new_line, "text":text, "target":target})
            }
            DiffRow::Delete { old_line, text } => {
                json!({"kind":"deleted", "old":old_line, "text":text, "target":target})
            }
            DiffRow::Context {
                old_line,
                new_line,
                text,
            } => {
                json!({"kind":"unchanged", "old":old_line, "new":new_line, "text":text, "target":false})
            }
            _ => unreachable!("a window only contains hunk content"),
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

/// Cut at context boundaries. Keep a contiguous before/after replacement paired.
/// Addition/deletion-only runs can also be cut at blank lines between declarations.
pub(super) struct Boundaries {
    pub(super) points: Vec<usize>,
    weights: Vec<usize>,
}

impl Boundaries {
    pub(super) fn new(rows: &[DiffRow]) -> Self {
        let mut points = vec![0];
        let mut index = 0;
        while index < rows.len() {
            let start = index;
            let changed = coordinate(&rows[index]).is_some();
            index += 1;
            if changed {
                while index < rows.len()
                    && coordinate(&rows[index]).is_some()
                    && !matches!(
                        (&rows[index - 1], &rows[index]),
                        (DiffRow::Add { .. }, DiffRow::Delete { .. })
                    )
                {
                    index += 1;
                }
                Self::split_single_side(rows, start..index, &mut points);
            }
            points.push(index);
        }
        let tokenizer = tiktoken_rs::o200k_base_singleton();
        let mut weights = vec![0];
        for row in rows {
            let tokens = tokenizer
                .encode_ordinary(&Window::row(row, true).to_string())
                .len();
            weights.push(weights.last().unwrap() + tokens);
        }
        Self { points, weights }
    }

    fn split_single_side(rows: &[DiffRow], range: Range<usize>, points: &mut Vec<usize>) {
        let side = coordinate(&rows[range.start]).unwrap().0;
        if rows[range.clone()]
            .iter()
            .any(|row| coordinate(row).unwrap().0 != side)
        {
            return;
        }
        for index in range.start + 1..range.end {
            let blank = match &rows[index - 1] {
                DiffRow::Add { text, .. } | DiffRow::Delete { text, .. } => {
                    text[1..].trim().is_empty()
                }
                _ => false,
            };
            if blank {
                points.push(index);
            }
        }
    }

    pub(super) fn midpoint(&self, range: &Range<usize>) -> Option<usize> {
        let midpoint =
            self.weights[range.start] + (self.weights[range.end] - self.weights[range.start]) / 2;
        self.points
            .iter()
            .copied()
            .filter(|point| *point > range.start && *point < range.end)
            .min_by_key(|point| self.weights[*point].abs_diff(midpoint))
    }
}
