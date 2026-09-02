//! Review state derived from stored repository baselines.

use review_repository::repository::{
    BaselineComparison, BaselineComparisonPlan, ChangedFile, FileKind, Interdiff, RepoPath,
    Repository, Snapshot, SnapshotId,
};
use review_store::{LoadResult, ReviewStore};

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

impl ReviewStatus {
    /// Return true when the file needs review work.
    pub fn needs_review(self) -> bool {
        self != Self::Reviewed
    }
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

/// A unified diff and the complete files on both sides.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewDiff {
    /// The unified diff bytes.
    pub unified: Vec<u8>,
    /// The complete file before the change, when available.
    pub old_content: Option<Vec<u8>>,
    /// The complete file after the change, when available.
    pub new_content: Option<Vec<u8>>,
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
    Compared {
        baseline_commit_id: String,
        diff: Vec<u8>,
    },
}

enum PlannedReviewState {
    Resolved(ReviewState),
    Compare { baseline_snapshot_id: SnapshotId },
}

impl ReviewComparison {
    fn state(&self) -> ReviewState {
        match self {
            Self::Unreviewed(warning) => ReviewState {
                status: ReviewStatus::Unreviewed,
                warning: *warning,
            },
            Self::Compared { diff, .. } => ReviewState {
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
        if identity.review_unit() != snapshot.identity.review_unit() {
            return Ok(MarkResult::ChangeChanged);
        }
        self.store.mark(
            identity.review_unit(),
            file.review_path().as_bytes(),
            identity.snapshot_id(),
        )?;
        Ok(MarkResult::Marked)
    }

    /// Derive the current review state of one changed path.
    pub fn status(&self, snapshot: &Snapshot, file: &ChangedFile) -> eyre::Result<ReviewState> {
        Ok(self.compare(snapshot, file)?.state())
    }

    /// Derive all path states with comparisons grouped by stored baseline.
    pub fn statuses(&self, snapshot: &Snapshot) -> eyre::Result<Vec<ReviewState>> {
        let review_unit = snapshot.identity.review_unit();
        let mut plan = BaselineComparisonPlan::default();
        let mut planned_states = Vec::with_capacity(snapshot.files.len());
        for file in &snapshot.files {
            let path = file.review_path().as_bytes();
            let planned_state = match self.store.load(review_unit, path)? {
                LoadResult::Unreviewed => PlannedReviewState::Resolved(ReviewState {
                    status: ReviewStatus::Unreviewed,
                    warning: None,
                }),
                LoadResult::UnknownSchema => PlannedReviewState::Resolved(ReviewState {
                    status: ReviewStatus::Unreviewed,
                    warning: Some(ReviewWarning::UnknownSchema),
                }),
                LoadResult::Reviewed(record) => {
                    let baseline_snapshot_id = SnapshotId::from(record.baseline_commit_id);
                    plan.add(baseline_snapshot_id.clone(), file.review_path().clone());
                    PlannedReviewState::Compare {
                        baseline_snapshot_id,
                    }
                }
            };
            planned_states.push(planned_state);
        }

        let results = self.repository.compare_baselines(snapshot, &plan)?;
        snapshot
            .files
            .iter()
            .zip(planned_states)
            .map(|(file, planned_state)| match planned_state {
                PlannedReviewState::Resolved(state) => Ok(state),
                PlannedReviewState::Compare {
                    baseline_snapshot_id,
                } => match results.get(&baseline_snapshot_id) {
                    Some(BaselineComparison::Missing) => {
                        self.store
                            .unreview(review_unit, file.review_path().as_bytes())?;
                        Ok(ReviewState {
                            status: ReviewStatus::Unreviewed,
                            warning: Some(ReviewWarning::BaselineExpired),
                        })
                    }
                    Some(BaselineComparison::Compared { changed_paths }) => Ok(ReviewState {
                        status: if changed_paths.contains(file.review_path()) {
                            ReviewStatus::ChangedSinceReview
                        } else {
                            ReviewStatus::Reviewed
                        },
                        warning: None,
                    }),
                    None => eyre::bail!("comparison plan omitted a stored baseline"),
                },
            })
            .collect()
    }

    /// Load the diff and both complete file versions for one changed path.
    pub fn diff(&self, snapshot: &Snapshot, file: &ChangedFile) -> eyre::Result<ReviewDiff> {
        match self.compare(snapshot, file)? {
            ReviewComparison::Unreviewed(_) => {
                let commit_id = snapshot.identity.snapshot_id();
                Ok(ReviewDiff {
                    unified: self.repository.diff(snapshot, file)?,
                    old_content: self.base_file_content(
                        snapshot,
                        file.old_path.as_ref(),
                        file.old_kind,
                    )?,
                    new_content: self.file_content(
                        commit_id,
                        file.new_path.as_ref(),
                        file.new_kind,
                    )?,
                })
            }
            ReviewComparison::Compared {
                baseline_commit_id,
                diff,
            } => {
                let kind = if file.old_kind == FileKind::File {
                    file.old_kind
                } else {
                    file.new_kind
                };
                Ok(ReviewDiff {
                    unified: diff,
                    old_content: self.file_content(
                        &baseline_commit_id,
                        Some(file.review_path()),
                        kind,
                    )?,
                    new_content: self.file_content(
                        snapshot.identity.snapshot_id(),
                        file.new_path.as_ref(),
                        file.new_kind,
                    )?,
                })
            }
        }
    }

    fn file_content(
        &self,
        revision: &str,
        path: Option<&RepoPath>,
        kind: FileKind,
    ) -> eyre::Result<Option<Vec<u8>>> {
        if kind != FileKind::File {
            return Ok(None);
        }
        path.map(|path| self.repository.file_at(revision, path))
            .transpose()
            .map_err(Into::into)
    }

    fn base_file_content(
        &self,
        snapshot: &Snapshot,
        path: Option<&RepoPath>,
        kind: FileKind,
    ) -> eyre::Result<Option<Vec<u8>>> {
        if kind != FileKind::File {
            return Ok(None);
        }
        path.map(|path| self.repository.base_file_at(snapshot, path))
            .transpose()
            .map_err(Into::into)
    }

    fn compare(&self, snapshot: &Snapshot, file: &ChangedFile) -> eyre::Result<ReviewComparison> {
        let review_unit = snapshot.identity.review_unit();
        let path = file.review_path().as_bytes();
        let record = match self.store.load(review_unit, path)? {
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
                self.store.unreview(review_unit, path)?;
                Ok(ReviewComparison::Unreviewed(Some(
                    ReviewWarning::BaselineExpired,
                )))
            }
            Interdiff::Diff(diff) => Ok(ReviewComparison::Compared {
                baseline_commit_id: record.baseline_commit_id,
                diff,
            }),
        }
    }

    /// Remove the review mark for one path.
    pub fn unreview(&self, snapshot: &Snapshot, file: &ChangedFile) -> eyre::Result<()> {
        self.store.unreview(
            snapshot.identity.review_unit(),
            file.review_path().as_bytes(),
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "review.tests.rs"]
mod tests;
