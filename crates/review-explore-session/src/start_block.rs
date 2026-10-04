//! Whether the reviewer can start a round: the review marks must leave something to review.
//! The session follows the rule of [`StartBlock`], and tells the pane and the page.

use review_explore::StartBlock;
use review_repository::repository::{PollResult, Snapshot};
use review_state::ReviewState;

use crate::ExploreSession;

impl ExploreSession {
    /// The review marks of the reviewer's `snapshot` changed, and leave its changed files in
    /// `states`: the pane and the page hear whether a round can start, when that changed, and
    /// the session reads the marks again for the mark tally of the change.
    pub fn marks_changed(&mut self, snapshot: &Snapshot, states: &[ReviewState]) {
        self.read_marks(snapshot);
        let block = StartBlock::of(states.iter().map(|state| state.status.needs_review()));
        if block != self.start_block {
            self.start_block = block;
            let _ = self.events.send(ui_events::ExploreStartBlock(block));
        }
        self.page.block_starts(block);
        self.publish_gain();
    }

    /// Reads the review marks of `snapshot` again, tells the pane and the page as
    /// [`Self::marks_changed`] does, and returns why a round cannot start on it.
    /// Marks that cannot be read block nothing: the prompt, which needs them too, says why.
    pub(crate) fn refresh_start_block(&mut self, snapshot: &Snapshot) -> Option<StartBlock> {
        let states = self.tracker.statuses(snapshot).ok()?;
        self.marks_changed(snapshot, &states);
        self.start_block
    }

    /// Why the kickoff about to go out must not: Jev, or another reviewer, marked the rest of
    /// the change since the start. A repository that cannot be read now blocks nothing.
    pub(crate) fn kickoff_block(&mut self) -> Option<StartBlock> {
        let Ok(PollResult::Complete(snapshot)) = self.repository.poll() else {
            return None;
        };
        self.refresh_start_block(&snapshot)
    }
}
