//! Grouped comparisons from stored review baselines.

use std::collections::{BTreeMap, BTreeSet};

use super::{RepoPath, SnapshotId};

/// Paths grouped by the stored baseline that reviewed them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BaselineComparisonPlan {
    baselines: BTreeMap<SnapshotId, BTreeSet<RepoPath>>,
}

impl BaselineComparisonPlan {
    /// Add one path to the comparison for its stored baseline.
    pub fn add(&mut self, baseline_snapshot_id: SnapshotId, path: RepoPath) {
        self.baselines
            .entry(baseline_snapshot_id)
            .or_default()
            .insert(path);
    }

    pub(super) fn baselines(&self) -> impl Iterator<Item = (&SnapshotId, &BTreeSet<RepoPath>)> {
        self.baselines.iter()
    }
}

/// The result of one grouped baseline comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BaselineComparison {
    /// The stored baseline no longer exists.
    Missing,
    /// The stored baseline exists, with the paths that changed from it.
    Compared { changed_paths: BTreeSet<RepoPath> },
}

/// Results indexed by stored baseline.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BaselineComparisonResults {
    baselines: BTreeMap<SnapshotId, BaselineComparison>,
}

impl BaselineComparisonResults {
    pub(super) fn insert(
        &mut self,
        baseline_snapshot_id: SnapshotId,
        comparison: BaselineComparison,
    ) {
        self.baselines.insert(baseline_snapshot_id, comparison);
    }

    /// Get the result for one baseline in the source plan.
    pub fn get(&self, baseline_snapshot_id: &SnapshotId) -> Option<&BaselineComparison> {
        self.baselines.get(baseline_snapshot_id)
    }
}

#[cfg(test)]
#[path = "comparison_plan.tests.rs"]
mod tests;
