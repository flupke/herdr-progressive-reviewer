//! How much of the change under review the review marks cover, for the Explore page's meter.
//!
//! The session keeps the marks of the reviewer's snapshot and reads them again whenever they
//! change: the reviewer's worker calls [`ExploreSession::marks_changed`] after each repository
//! refresh (which the repository watcher starts on filesystem events, and which follows each
//! run of Jev), after each mark by hand and once a round ends; and the session reads them again
//! itself after it applies an answer's marks and after it gives back the marks of a cancelled
//! answer. The round and the question it waits for come from the session's state, so the
//! tally is right for the stage the page shows.

use review_explore_tally::MarkTally;
use review_repository::repository::Snapshot;

use crate::ExploreSession;

impl ExploreSession {
    /// The review marks of the change under review, in the round the session shows, with what
    /// answering the question the round waits for adds. While the session shows an earlier
    /// round, which the reviewer can only Reset, that round's marks are the round's and the
    /// marks of the newer round count as another round's.
    pub fn mark_tally(&self) -> MarkTally {
        let round = self.state.round.as_ref();
        // An earlier round, which the reviewer can only Reset, takes no answer.
        let question = round
            .filter(|_| !self.state.historical)
            .and_then(|round| round.exploration.waiting_turn());
        self.marks.mark_tally(round, question)
    }

    /// Read the review marks of `snapshot` again.
    pub(crate) fn read_marks(&mut self, snapshot: &Snapshot) {
        self.marks.read(&self.tracker, snapshot);
    }
}
