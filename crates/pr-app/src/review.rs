//! Review state derived from stored baselines and jj interdiffs.

use pr_core::repository::{ChangedFile, Interdiff, Repository, Snapshot};
use pr_state::{LoadResult, ReviewStore};

/// The review state of one changed path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewStatus {
    /// The path has no usable review mark.
    Unreviewed,
    /// The path is unchanged from its review baseline.
    Reviewed,
    /// The path changed after its review baseline.
    ChangedSinceReview,
}

/// A non-fatal warning found while deriving review state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewWarning {
    /// The stored record uses an unknown schema.
    UnknownSchema,
    /// The stored baseline commit no longer exists.
    BaselineExpired,
}

/// Derived state and its optional storage warning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReviewState {
    /// The path review state.
    pub status: ReviewStatus,
    /// A non-fatal warning that the UI must show.
    pub warning: Option<ReviewWarning>,
}

/// The result of a request to mark one path as reviewed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkResult {
    /// The review mark was stored.
    Marked,
    /// The user moved to a different change before the mark.
    ChangeChanged,
}

#[derive(Debug, Eq, PartialEq)]
enum ReviewComparison {
    Unreviewed(Option<ReviewWarning>),
    Compared(Vec<u8>),
}

impl ReviewComparison {
    fn state(&self) -> ReviewState {
        match self {
            Self::Unreviewed(warning) => ReviewState {
                status: ReviewStatus::Unreviewed,
                warning: *warning,
            },
            Self::Compared(diff) => ReviewState {
                status: if diff.is_empty() {
                    ReviewStatus::Reviewed
                } else {
                    ReviewStatus::ChangedSinceReview
                },
                warning: None,
            },
        }
    }
}

/// Review operations for one repository.
#[derive(Debug)]
pub struct ReviewTracker {
    repository: Repository,
    store: ReviewStore,
}

impl ReviewTracker {
    /// Connect a repository to its on-disk review store.
    pub fn new(repository: Repository, store: ReviewStore) -> Self {
        Self { repository, store }
    }

    /// Mark one path at the current exact commit.
    pub fn mark(&self, snapshot: &Snapshot, file: &ChangedFile) -> eyre::Result<MarkResult> {
        let identity = self.repository.current_identity()?;
        if identity.change_id != snapshot.identity.change_id {
            return Ok(MarkResult::ChangeChanged);
        }
        self.store.mark(
            identity.change_id.as_str(),
            file.review_path().as_bytes(),
            identity.commit_id.as_str(),
        )?;
        Ok(MarkResult::Marked)
    }

    /// Derive the current review state of one changed path.
    pub fn status(&self, snapshot: &Snapshot, file: &ChangedFile) -> eyre::Result<ReviewState> {
        Ok(self.compare(snapshot, file)?.state())
    }

    pub(crate) fn diff(&self, snapshot: &Snapshot, file: &ChangedFile) -> eyre::Result<Vec<u8>> {
        match self.compare(snapshot, file)? {
            ReviewComparison::Unreviewed(_) => Ok(self.repository.diff(snapshot, file)?),
            ReviewComparison::Compared(diff) => Ok(diff),
        }
    }

    fn compare(&self, snapshot: &Snapshot, file: &ChangedFile) -> eyre::Result<ReviewComparison> {
        let change_id = snapshot.identity.change_id.as_str();
        let path = file.review_path().as_bytes();
        let record = match self.store.load(change_id, path)? {
            LoadResult::Unreviewed => return Ok(ReviewComparison::Unreviewed(None)),
            LoadResult::UnknownSchema => {
                return Ok(ReviewComparison::Unreviewed(Some(
                    ReviewWarning::UnknownSchema,
                )));
            }
            LoadResult::Reviewed(record) => record,
        };

        match self
            .repository
            .interdiff(&record.baseline_commit_id, snapshot, file.review_path())?
        {
            Interdiff::MissingBaseline => {
                self.store.unreview(change_id, path)?;
                Ok(ReviewComparison::Unreviewed(Some(
                    ReviewWarning::BaselineExpired,
                )))
            }
            Interdiff::Diff(diff) => Ok(ReviewComparison::Compared(diff)),
        }
    }

    /// Remove the review mark for one path.
    pub fn unreview(&self, snapshot: &Snapshot, file: &ChangedFile) -> eyre::Result<()> {
        self.store.unreview(
            snapshot.identity.change_id.as_str(),
            file.review_path().as_bytes(),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pr_core::repository::PollResult;
    use pr_tests::{JjFixture, JjLayout};

    use super::*;

    fn snapshot(repository: &Repository) -> Snapshot {
        match repository.poll().unwrap() {
            PollResult::Complete(snapshot) => snapshot,
            PollResult::ChangedDuringPoll => panic!("test repository changed during a poll"),
        }
    }

    #[test]
    fn diff_uses_the_review_baseline() {
        let jj = JjFixture::new(JjLayout::NonColocated);
        jj.write("reviewed.txt", b"before\n");
        jj.new_change("review");
        jj.write("reviewed.txt", b"after\n");
        let repository = Repository::discover(jj.root()).unwrap();
        let state = tempfile::tempdir().unwrap();
        let tracker = ReviewTracker::new(
            repository.clone(),
            ReviewStore::open(state.path(), jj.root()).unwrap(),
        );
        let reviewed = snapshot(&repository);
        tracker.mark(&reviewed, &reviewed.files[0]).unwrap();

        jj.write("reviewed.txt", b"after\nfoo\n");
        let changed = snapshot(&repository);
        let diff = String::from_utf8(tracker.diff(&changed, &changed.files[0]).unwrap()).unwrap();

        assert!(diff.contains("+foo"));
        assert!(!diff.contains("+after"));
    }
}
