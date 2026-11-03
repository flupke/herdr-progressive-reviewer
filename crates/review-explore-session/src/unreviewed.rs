//! The unreviewed lines of each prompt, read from the review marks and
//! written as numbered diffs.

use eyre::WrapErr;
use review_explore_runner::Unreviewed;
use review_repository::repository::{PollResult, Snapshot};
use review_state::{ReviewStatus, ReviewTracker};

use crate::ExploreSession;
use crate::unreviewed_diffs::UnreviewedDiffs;

impl ExploreSession {
    /// Write what no review mark covers in the current snapshot as numbered
    /// diffs, and say where they are. A prompt is not sent without them.
    pub(crate) fn unreviewed(&mut self) -> eyre::Result<Unreviewed> {
        // An earlier prompt's diffs go: each prompt has a directory of its own.
        self.diffs = None;
        let snapshot = self
            .complete_snapshot()
            .wrap_err("Explore could not write the unreviewed diffs")?;
        let (unreviewed, diffs) = self
            .write_unreviewed(&self.tracker, &snapshot)
            .wrap_err("Explore could not write the unreviewed diffs")?;
        self.diffs = Some(diffs);
        // The prompt numbers the lines like this snapshot: the turn it asks for keeps the
        // files that differ from the round's here.
        self.prompt_diverged = self.diverged_at(&snapshot);
        Ok(unreviewed)
    }

    /// The current snapshot, once the repository finished loading it.
    pub(crate) fn complete_snapshot(&self) -> eyre::Result<Snapshot> {
        let PollResult::Complete(snapshot) = self.repository.poll()? else {
            eyre::bail!("the repository is still loading");
        };
        Ok(snapshot)
    }

    /// Writes what no review mark of `tracker` covers in `snapshot`, in a directory of its own.
    pub(crate) fn write_unreviewed(
        &self,
        tracker: &ReviewTracker,
        snapshot: &Snapshot,
    ) -> eyre::Result<(Unreviewed, UnreviewedDiffs)> {
        let states = tracker.statuses(snapshot)?;
        let moved = self.state.comparison.as_ref().is_some_and(|comparison| {
            !comparison.checkpoint.matches(
                snapshot.identity.review_unit(),
                snapshot.identity.snapshot_id(),
            )
        });
        let open: Vec<_> = snapshot
            .files
            .iter()
            .zip(states)
            .filter(|(_, state)| state.status != ReviewStatus::Reviewed)
            .map(|(file, _)| file)
            .collect();
        let diffs = UnreviewedDiffs::write(tracker, snapshot, open.iter().copied())?;
        let unreviewed = Unreviewed {
            directory: diffs.directory().to_owned(),
            files: open.len(),
            index: diffs.index(),
            notice: moved.then(|| {
                "the code changed since this round started: these lines are numbered like the \
                 current code, and this turn's review marks apply only on the files whose diff \
                 has not changed since the round started"
                    .into()
            }),
        };
        Ok((unreviewed, diffs))
    }
}
