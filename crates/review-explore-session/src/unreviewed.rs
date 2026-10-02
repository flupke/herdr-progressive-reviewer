//! The unreviewed lines of each prompt, read from the review marks and
//! written as numbered diffs.

use eyre::WrapErr;
use review_explore_runner::Unreviewed;
use review_repository::repository::PollResult;
use review_state::ReviewStatus;

use crate::ExploreSession;
use crate::unreviewed_diffs::UnreviewedDiffs;

impl ExploreSession {
    /// Write what no review mark covers in the current snapshot as numbered
    /// diffs, and say where they are. A prompt is not sent without them.
    pub(crate) fn unreviewed(&mut self) -> eyre::Result<Unreviewed> {
        // An earlier prompt's diffs go: each prompt has a directory of its own.
        self.diffs = None;
        self.write_unreviewed()
            .wrap_err("Explore could not write the unreviewed diffs")
    }

    fn write_unreviewed(&mut self) -> eyre::Result<Unreviewed> {
        let PollResult::Complete(snapshot) = self.repository.poll()? else {
            eyre::bail!("the repository is still loading");
        };
        let states = self.tracker.statuses(&snapshot)?;
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
        let diffs = UnreviewedDiffs::write(&self.tracker, &snapshot, open.iter().copied())?;
        let unreviewed = Unreviewed {
            directory: diffs.directory().to_owned(),
            files: open.len(),
            index: diffs.index(),
            notice: moved.then(|| {
                "the code changed since this round started: these lines are numbered like the \
                 current code, and review marks are not applied until a new round starts"
                    .into()
            }),
        };
        self.diffs = Some(diffs);
        Ok(unreviewed)
    }
}
