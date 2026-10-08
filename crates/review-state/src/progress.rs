//! Review progress across a change, weighted by changed lines.

use review_repository::repository::DiffStatistics;

use crate::{ReviewState, ReviewStatus};

/// Changed lines reviewed so far across the files of one change.
///
/// Each file weighs its changed line count, and at least one line so binary
/// and pure-rename files still count. A file that still needs review keeps at
/// least one remaining line, so the change only reaches 100% once every file
/// is reviewed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReviewProgress {
    reviewed_lines: u64,
    total_lines: u64,
}

impl ReviewProgress {
    /// Add one file, given its changed lines and its review state.
    fn add(&mut self, statistics: DiffStatistics, state: &ReviewState) {
        let total = changed_lines(statistics);
        let remaining = match state.status {
            ReviewStatus::Reviewed => 0,
            ReviewStatus::Unreviewed => total,
            ReviewStatus::ChangedSinceReview | ReviewStatus::PartiallyReviewed => {
                changed_lines(state.current_diff_statistics).min(total)
            }
        };
        self.reviewed_lines += total - remaining;
        self.total_lines += total;
    }

    /// Whether the change has no changed line to review.
    pub fn is_empty(self) -> bool {
        self.total_lines == 0
    }

    /// The reviewed share in whole percent, rounded down.
    pub fn percent(self) -> u64 {
        self.share_of(100)
    }

    /// How many of `cells` a progress bar fills, rounded down.
    pub fn filled(self, cells: usize) -> usize {
        let filled = self.share_of(u64::try_from(cells).unwrap_or(u64::MAX));
        usize::try_from(filled).unwrap_or(cells).min(cells)
    }

    /// The reviewed share of `scale`, rounded down, or zero for an empty change.
    fn share_of(self, scale: u64) -> u64 {
        self.reviewed_lines
            .saturating_mul(scale)
            .checked_div(self.total_lines)
            .unwrap_or_default()
    }
}

impl<'a> FromIterator<(DiffStatistics, &'a ReviewState)> for ReviewProgress {
    fn from_iter<I: IntoIterator<Item = (DiffStatistics, &'a ReviewState)>>(files: I) -> Self {
        let mut progress = Self::default();
        for (statistics, state) in files {
            progress.add(statistics, state);
        }
        progress
    }
}

/// Changed lines of one file, counting at least one.
fn changed_lines(statistics: DiffStatistics) -> u64 {
    (statistics.lines_added + statistics.lines_removed).max(1)
}

#[cfg(test)]
#[path = "progress.tests.rs"]
mod tests;
