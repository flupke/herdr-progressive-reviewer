use std::ops::Range;

use review_repository::diff::DiffRow;
use serde::Serialize;
use serde_json::Value;

use super::super::{Candidate, MAX_STATE_BYTES};
use super::super::{optimized::SplitPlan, sections::Sections};
use super::{dataset::Fixture, study::RequestProfile, window::Window};

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
        if self == Self::RecursiveOverlap
            && profile.is_some_and(RequestProfile::is_evaluated_checklist)
        {
            return super::super::optimized::SourceFile {
                id: fixture.case.id.clone(),
                file_index: 0,
                path: &fixture.case.path,
                language: &fixture.case.language,
                context: fixture.case.context.clone(),
                hunks: &fixture.hunks,
                hunk_starts: Some(&fixture.hunk_starts),
            }
            .prepare_with(budget, profile.expect("checked profile").format())
            .into_iter()
            .map(|prepared| Chunk {
                candidate: prepared.candidate,
                body: prepared.body,
                estimated_tokens: prepared.estimated_tokens,
                oversized: prepared.oversized,
            })
            .collect();
        }
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
                    profile,
                };
                let splitter = SplitPlan::new(&rows);
                let mut targets = Vec::new();
                if self == Self::WholeHunk {
                    targets.push(0..rows.len());
                } else {
                    splitter.divide(0..rows.len(), &mut targets, &|target, context| {
                        planner.fits(target, context)
                    });
                }
                targets
                    .into_iter()
                    .map(|target| {
                        let context = if self == Self::RecursiveOverlap {
                            splitter
                                .overlap(&target, &|target, context| planner.fits(target, context))
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
}
