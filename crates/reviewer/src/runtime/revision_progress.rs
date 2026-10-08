//! The reviewed share of each revision of the history the revision selector shows. The worker
//! computes them after the history, one revision at a time, so that the selector shows at once
//! and other repository work waits for one revision at most. A share stays known while the
//! revision keeps its commit and its marks keep their stamp, which the review store changes with
//! each mark, from any process: those come at once, and only the others are computed.

use std::collections::{HashMap, VecDeque};

use review_repository::repository::{ChangeId, Repository, RevisionHistoryLine, SnapshotId};
use review_state::{ReviewProgress, ReviewTracker};
use review_store::ReviewStore;
use ui_events::RevisionHistoryLoadId;

/// The revisions of the latest loaded history whose share is still to compute, whether the
/// worker's command for the next one is queued (one at a time, so that a command that comes
/// meanwhile waits for one revision at most), and the shares known so far.
#[derive(Debug, Default)]
pub(super) struct HistoryProgress {
    load_id: Option<RevisionHistoryLoadId>,
    revisions: VecDeque<ChangeId>,
    queued: bool,
    known: HashMap<ChangeId, Known>,
}

/// A share as it was computed: valid while the revision's commit and its marks' stamp are the
/// same.
#[derive(Debug)]
struct Known {
    commit: SnapshotId,
    stamp: Option<String>,
    progress: ReviewProgress,
}

/// What a new history starts with.
pub(super) struct Started {
    /// The shares still valid, to show at once.
    pub(super) known: Vec<(ChangeId, ReviewProgress)>,
    /// Whether to queue the worker's command for the others.
    pub(super) queue: bool,
}

impl HistoryProgress {
    /// Starts on the revisions of `lines` that the selector can open, nearest to the current one
    /// first, since the reviewer reads around it; they replace those of an earlier history.
    pub(super) fn start(
        &mut self,
        load_id: RevisionHistoryLoadId,
        lines: &[RevisionHistoryLine],
        store: &ReviewStore,
    ) -> Started {
        let current = lines.iter().position(|line| line.is_current).unwrap_or(0);
        let mut rows = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !line.is_immutable && line.change_id.is_some())
            .collect::<Vec<_>>();
        rows.sort_by_key(|(row, _)| row.abs_diff(current));
        self.load_id = Some(load_id);
        self.revisions.clear();
        // Shares of revisions the history no longer shows go: they were rewritten or abandoned.
        self.known.retain(|change_id, _| {
            rows.iter()
                .any(|(_, line)| line.change_id.as_ref() == Some(change_id))
        });
        let mut known = Vec::new();
        for (_, line) in rows {
            let Some(change_id) = line.change_id.clone() else {
                continue;
            };
            match self.still_known(&change_id, line.commit_id.as_ref(), store) {
                Some(progress) => known.push((change_id, progress)),
                None => self.revisions.push_back(change_id),
            }
        }
        Started {
            known,
            queue: self.queue(),
        }
    }

    /// The share of `change_id`, if it was computed at `commit` with the marks as they are.
    fn still_known(
        &self,
        change_id: &ChangeId,
        commit: Option<&SnapshotId>,
        store: &ReviewStore,
    ) -> Option<ReviewProgress> {
        let known = self.known.get(change_id)?;
        let stamp = store.marks_stamp(change_id.review_unit()).ok()?;
        (Some(&known.commit) == commit && known.stamp == stamp).then_some(known.progress)
    }

    /// The next revision to compute, as the worker's queued command runs.
    pub(super) fn next(&mut self) -> Option<(RevisionHistoryLoadId, ChangeId)> {
        self.queued = false;
        Some((self.load_id?, self.revisions.pop_front()?))
    }

    /// Whether to queue the worker's command for the next revision: when one is left and none
    /// is queued.
    pub(super) fn queue(&mut self) -> bool {
        let queue = !self.queued && !self.revisions.is_empty();
        self.queued |= queue;
        queue
    }

    /// How much of the revision of `change_id` its review marks cover, as the status line counts
    /// it for the current revision; none when the revision cannot be read. The share is kept
    /// with the stamp the marks had before it was computed, so that a mark made meanwhile makes
    /// the next history compute it again.
    pub(super) fn compute(
        &mut self,
        repository: &Repository,
        tracker: &ReviewTracker,
        store: &ReviewStore,
        change_id: &ChangeId,
    ) -> Option<ReviewProgress> {
        // A stamp that cannot be read lets the share be computed, not kept.
        let stamp = store.marks_stamp(change_id.review_unit());
        let snapshot = repository.snapshot_of(change_id).ok()??;
        let states = tracker.statuses(&snapshot).ok()?;
        let progress = snapshot
            .files
            .iter()
            .zip(&states)
            .map(|(file, state)| (file.statistics, state))
            .collect();
        if let Ok(stamp) = stamp {
            self.known.insert(
                change_id.clone(),
                Known {
                    commit: SnapshotId::from(snapshot.identity.snapshot_id().to_owned()),
                    stamp,
                    progress,
                },
            );
        }
        Some(progress)
    }
}

#[cfg(test)]
#[path = "revision_progress.tests.rs"]
mod tests;
