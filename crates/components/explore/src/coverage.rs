//! Coverage display data is refreshed by pass events, never while drawing.
use review_explore::{
    ChangedLineCoverage, Comparison, CoverageLedger, CoverageSummary, CoverageUnit, FileCoverage,
    Gap,
};

#[derive(Default)]
pub(super) struct CoverageCache {
    current: Option<CachedCoverage>,
    #[cfg(test)]
    rebuilds: usize,
}

struct CachedCoverage {
    instance: String,
    counts_revision: u64,
    snapshot: CoverageSnapshot,
}

pub(super) struct CoverageSnapshot {
    pub(super) summary: CoverageSummary,
    pub(super) lines: ChangedLineCoverage,
    pub(super) files: Vec<CachedFileCoverage>,
    pub(super) remaining: Vec<CoverageUnit>,
    pub(super) exclusions: Vec<Gap>,
    pub(super) exclusions_enabled: bool,
    pub(super) total_lines: u64,
    pub(super) filtered_lines: u64,
}

pub(super) struct CachedFileCoverage {
    pub(super) coverage: FileCoverage,
    pub(super) lines: ChangedLineCoverage,
}

impl CoverageCache {
    pub(super) fn refresh(
        &mut self,
        instance: &str,
        coverage: &CoverageLedger,
        comparison: &Comparison,
        exclusions_enabled: bool,
    ) {
        if self.current.as_ref().is_some_and(|current| {
            current.instance == instance
                && current.counts_revision == coverage.counts_revision
                && current.snapshot.exclusions_enabled == exclusions_enabled
        }) {
            return;
        }
        self.current = Some(CachedCoverage {
            instance: instance.into(),
            counts_revision: coverage.counts_revision,
            snapshot: CoverageSnapshot::new(coverage, comparison, exclusions_enabled),
        });
        #[cfg(test)]
        {
            self.rebuilds += 1;
        }
    }

    pub(super) fn get(&self) -> Option<&CoverageSnapshot> {
        self.current.as_ref().map(|current| &current.snapshot)
    }
}

impl CoverageSnapshot {
    fn new(coverage: &CoverageLedger, comparison: &Comparison, exclusions_enabled: bool) -> Self {
        let file_lines = coverage.required_changed_line_counts(comparison, exclusions_enabled);
        let lines = ChangedLineCoverage {
            explored: file_lines.iter().map(|lines| lines.explored).sum(),
            total: file_lines.iter().map(|lines| lines.total).sum(),
        };
        Self {
            summary: coverage.summary(exclusions_enabled),
            lines,
            files: coverage
                .files(comparison, exclusions_enabled)
                .into_iter()
                .zip(file_lines)
                .map(|(coverage, lines)| CachedFileCoverage { coverage, lines })
                .collect(),
            remaining: coverage.remaining(exclusions_enabled),
            exclusions: coverage.exclusion_gaps(comparison),
            exclusions_enabled,
            total_lines: coverage.changed_line_coverage(None).total,
            filtered_lines: coverage.jev_filtered_changed_lines(exclusions_enabled),
        }
    }
}

#[cfg(test)]
#[path = "coverage.tests.rs"]
mod tests;
