//! Review state derived from stored repository baselines.

use pr_core::repository::{ChangedFile, FileKind, Interdiff, RepoPath, Repository, Snapshot};
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

/// A unified diff and the complete files on both sides.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewDiff {
    pub(crate) unified: Vec<u8>,
    pub(crate) old_content: Option<Vec<u8>>,
    pub(crate) new_content: Option<Vec<u8>>,
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
        if identity.review_id() != snapshot.identity.review_id() {
            return Ok(MarkResult::ChangeChanged);
        }
        self.store.mark(
            identity.review_id(),
            file.review_path().as_bytes(),
            identity.snapshot_id(),
        )?;
        Ok(MarkResult::Marked)
    }

    /// Derive the current review state of one changed path.
    pub fn status(&self, snapshot: &Snapshot, file: &ChangedFile) -> eyre::Result<ReviewState> {
        Ok(self.compare(snapshot, file)?.state())
    }

    pub(crate) fn diff(&self, snapshot: &Snapshot, file: &ChangedFile) -> eyre::Result<ReviewDiff> {
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
        let change_id = snapshot.identity.review_id();
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
            Interdiff::Diff(diff) => Ok(ReviewComparison::Compared {
                baseline_commit_id: record.baseline_commit_id,
                diff,
            }),
        }
    }

    /// Remove the review mark for one path.
    pub fn unreview(&self, snapshot: &Snapshot, file: &ChangedFile) -> eyre::Result<()> {
        self.store
            .unreview(snapshot.identity.review_id(), file.review_path().as_bytes())?;
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
        let initial = tracker.diff(&reviewed, &reviewed.files[0]).unwrap();
        assert_eq!(initial.old_content.as_deref(), Some(&b"before\n"[..]));
        assert_eq!(initial.new_content.as_deref(), Some(&b"after\n"[..]));
        tracker.mark(&reviewed, &reviewed.files[0]).unwrap();

        jj.write("reviewed.txt", b"after\nfoo\n");
        let changed = snapshot(&repository);
        let loaded = tracker.diff(&changed, &changed.files[0]).unwrap();
        let diff = String::from_utf8(loaded.unified).unwrap();

        assert!(diff.contains("+foo"));
        assert!(!diff.contains("+after"));
        assert_eq!(loaded.old_content.as_deref(), Some(&b"after\n"[..]));
        assert_eq!(loaded.new_content.as_deref(), Some(&b"after\nfoo\n"[..]));
    }
}
