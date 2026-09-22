use std::ops::Range;

use review_repository::diff::DiffRow;
use serde::Serialize;
use serde_json::Value;

use super::super::{Candidate, MAX_STATE_BYTES};
use super::{
    dataset::{Fixture, coordinate},
    sections::Sections,
    study::RequestProfile,
    window::{Boundaries, Window},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Strategy {
    LegacyBlocks,
    WholeHunk,
    Recursive,
    RecursiveOverlap,
}

impl Strategy {
    pub(super) const ALL: [Self; 4] = [
        Self::LegacyBlocks,
        Self::WholeHunk,
        Self::Recursive,
        Self::RecursiveOverlap,
    ];

    pub(super) fn plan(self, fixture: &Fixture, budget: usize) -> Vec<Chunk> {
        self.plan_with(fixture, budget, None)
    }

    pub(super) fn plan_with(
        self,
        fixture: &Fixture,
        budget: usize,
        profile: Option<&RequestProfile>,
    ) -> Vec<Chunk> {
        if self == Self::LegacyBlocks {
            let mut candidates = Vec::new();
            Candidate::prepare_file(0, &fixture.file, fixture.patch.as_bytes(), &mut candidates);
            return candidates
                .into_iter()
                .map(|candidate| {
                    let body = candidate.request();
                    let too_large = candidate.state.to_string().len() > MAX_STATE_BYTES;
                    Chunk::new(candidate, body, budget, too_large)
                })
                .collect();
        }
        fixture
            .hunks
            .iter()
            .flat_map(|rows| {
                let rows = if fixture.case.language == "Rust" {
                    Sections::align(rows)
                } else {
                    rows.clone()
                };
                let planner = Planner {
                    fixture,
                    rows: &rows,
                    budget,
                    boundaries: Boundaries::new(&rows),
                    profile,
                };
                let mut targets = Vec::new();
                if self == Self::WholeHunk {
                    targets.push(0..rows.len());
                } else {
                    planner.divide(0..rows.len(), &mut targets);
                }
                targets
                    .into_iter()
                    .map(|target| {
                        let context = if self == Self::RecursiveOverlap {
                            planner.overlap(&target)
                        } else {
                            target.clone()
                        };
                        planner.chunk(target, context)
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

pub(super) struct Chunk {
    pub(super) candidate: Candidate,
    pub(super) body: Value,
    pub(super) estimated_tokens: usize,
    pub(super) oversized: bool,
}

impl Chunk {
    fn new(candidate: Candidate, body: Value, budget: usize, legacy_oversized: bool) -> Self {
        let estimated_tokens = tiktoken_rs::o200k_base_singleton()
            .encode_ordinary(&body.to_string())
            .len();
        Self {
            candidate,
            body,
            estimated_tokens,
            oversized: legacy_oversized || estimated_tokens > budget,
        }
    }
}

struct Planner<'a> {
    fixture: &'a Fixture,
    rows: &'a [DiffRow],
    budget: usize,
    boundaries: Boundaries,
    profile: Option<&'a RequestProfile>,
}

impl Planner<'_> {
    fn chunk(&self, target: Range<usize>, context: Range<usize>) -> Chunk {
        let candidate = Window {
            case: &self.fixture.case,
            rows: self.rows,
            target,
            context,
        }
        .candidate();
        if let Some(profile) = self.profile {
            let body = profile.request(&candidate, &self.fixture.case);
            let estimated_tokens = RequestProfile::tokens(&body);
            Chunk {
                candidate,
                body,
                estimated_tokens,
                oversized: estimated_tokens > self.budget,
            }
        } else {
            let body = Window::request(&candidate);
            Chunk::new(candidate, body, self.budget, false)
        }
    }

    fn fits(&self, target: &Range<usize>, context: Range<usize>) -> bool {
        !self.chunk(target.clone(), context).oversized
    }

    fn divide(&self, target: Range<usize>, leaves: &mut Vec<Range<usize>>) {
        if !self.rows[target.clone()]
            .iter()
            .any(|row| coordinate(row).is_some())
        {
            return;
        }
        if self.fits(&target, target.clone()) {
            leaves.push(target);
        } else if let Some(midpoint) = self.boundaries.midpoint(&target) {
            self.divide(target.start..midpoint, leaves);
            self.divide(midpoint..target.end, leaves);
        } else {
            // An indivisible replacement can remain oversized; never drop its targets.
            leaves.push(target);
        }
    }

    fn overlap(&self, target: &Range<usize>) -> Range<usize> {
        if !self.fits(target, target.clone()) {
            return target.clone();
        }
        let points = &self.boundaries.points;
        let left = points.binary_search(&target.start).unwrap();
        let right = points.binary_search(&target.end).unwrap();
        let spare = left + points.len() - right - 1;
        let width = Self::largest(0, spare, |extra| {
            let range = self.balanced(left, right, extra);
            self.fits(target, range)
        });
        let mut range = self.balanced(left, right, width);
        let current = points.binary_search(&range.start).unwrap();
        let extension = Self::largest(0, current, |extra| {
            self.fits(target, points[current - extra]..range.end)
        });
        range.start = points[current - extension];
        let current = points.binary_search(&range.end).unwrap();
        let extension = Self::largest(0, points.len() - current - 1, |extra| {
            self.fits(target, range.start..points[current + extra])
        });
        range.end = points[current + extension];
        range
    }

    fn balanced(&self, left: usize, right: usize, extra: usize) -> Range<usize> {
        let points = &self.boundaries.points;
        let right_capacity = points.len() - right - 1;
        let left_extra = extra
            .div_ceil(2)
            .min(left)
            .max(extra.saturating_sub(right_capacity));
        points[left - left_extra]..points[right + extra - left_extra]
    }

    fn largest(mut low: usize, mut high: usize, fits: impl Fn(usize) -> bool) -> usize {
        while low < high {
            let midpoint = low + (high - low).div_ceil(2);
            if fits(midpoint) {
                low = midpoint;
            } else {
                high = midpoint - 1;
            }
        }
        low
    }
}
