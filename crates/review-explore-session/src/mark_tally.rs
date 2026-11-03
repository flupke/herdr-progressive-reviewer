//! How much of the change under review the review marks cover, for the Explore page's meter.
//!
//! The session keeps the marks of the reviewer's snapshot and reads them again whenever they
//! change: the reviewer's worker calls [`ExploreSession::marks_changed`] after each repository
//! refresh (which the repository watcher starts on filesystem events, and which follows each
//! run of Jev), after each mark by hand and once a round ends; and the session reads them again
//! itself after it applies an answer's marks, after it gives back the marks of a cancelled
//! answer and after Implement reopens the marks of a round concluded at the drawing board. The
//! round and the question it waits for come from the session's state, so the tally is right for
//! the stage the page shows. The page hears the tally with each stage the session publishes and
//! each time the marks change.

use std::cell::RefCell;

use review_explore::{ExploreRound, MarkableFiles, TurnRequest};
use review_explore_tally::MarkTally;
use review_repository::repository::{Snapshot, SnapshotIdentity};

use crate::ExploreSession;

/// The files on which the marks of a round apply at a snapshot, worked out once per round and
/// snapshot, since that reads the diff of each changed file once the code changed.
#[derive(Debug, Default)]
pub(crate) struct MarkableCache(RefCell<Option<MarkableAt>>);

#[derive(Debug)]
struct MarkableAt {
    /// The round's instance.
    instance: String,
    snapshot: SnapshotIdentity,
    files: MarkableFiles,
}

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
        let markable = match (round, question, self.marks_snapshot.as_ref()) {
            (Some(round), Some(question), Some(snapshot)) => {
                self.turn_markable(round, &question.request, snapshot)
            }
            _ => MarkableFiles::All,
        };
        self.marks.mark_tally(round, question, &markable)
    }

    /// Read the review marks of `snapshot` again.
    pub(crate) fn read_marks(&mut self, snapshot: &Snapshot) {
        self.marks.read(&self.tracker, snapshot);
        self.marks_snapshot = Some(snapshot.clone());
    }

    /// The files of `snapshot` on which the marks of `round` apply.
    pub(crate) fn markable(&self, round: &ExploreRound, snapshot: &Snapshot) -> MarkableFiles {
        if round.is_at(&snapshot.identity) {
            return MarkableFiles::All;
        }
        let mut cache = self.markable.0.borrow_mut();
        let cached = cache.as_ref().filter(|cached| {
            cached.instance == round.exploration.instance && cached.snapshot == snapshot.identity
        });
        if let Some(cached) = cached {
            return cached.files.clone();
        }
        let files = MarkableFiles::at(&round.exploration.comparison, &self.repository, snapshot);
        // A diff that could not be read is read again next time.
        if files.complete() {
            *cache = Some(MarkableAt {
                instance: round.exploration.instance.clone(),
                snapshot: snapshot.identity.clone(),
                files: files.clone(),
            });
        }
        files
    }

    /// The files of `snapshot` on which the marks of the agent's turn `request` of `round`
    /// apply: not those whose diff had diverged when the round accepted the turn.
    pub(crate) fn turn_markable(
        &self,
        round: &ExploreRound,
        request: &str,
        snapshot: &Snapshot,
    ) -> MarkableFiles {
        let diverged = round
            .turns
            .get(request)
            .map_or(&[][..], |turn| turn.diverged.as_slice());
        self.markable(round, snapshot).excluding(snapshot, diverged)
    }

    /// The files whose diff differs from the round's at `snapshot`, by review path.
    pub(crate) fn diverged_at(&self, snapshot: &Snapshot) -> Vec<String> {
        let Some(round) = self.state.round.as_ref() else {
            return Vec::new();
        };
        self.markable(round, snapshot)
            .diverged(&round.exploration.comparison, snapshot)
    }

    /// The files whose diff differs from the round's now, by review path; none when the
    /// repository cannot tell, as while it changes: the prompt's own snapshot told already.
    pub(crate) fn diverged_now(&self) -> Vec<String> {
        // A poll that meets a file being saved is tried again.
        (0..3)
            .find_map(|_| self.complete_snapshot().ok())
            .map(|snapshot| self.diverged_at(&snapshot))
            .unwrap_or_default()
    }

    /// Keeps with the turn `request` the files whose diff differed from the round's when its
    /// prompt was written, which its marks never reach.
    pub(crate) fn record_diverged(&mut self, request: &TurnRequest) {
        let diverged = self
            .prompted_diverged
            .remove(&request.request)
            .unwrap_or_default();
        if diverged.is_empty() {
            return;
        }
        let checkpoint = &request.checkpoint;
        let saved = self
            .rounds
            .update(&checkpoint.review_unit, &request.instance, |round| {
                if let Some(turn) = round.turns.get_mut(&request.request) {
                    crate::records::merge_diverged(&mut turn.diverged, diverged.clone());
                }
                Ok(())
            });
        match saved {
            Ok(((), round)) => self.state.round = Some(round),
            Err(error) => {
                let _ = self
                    .events
                    .send(ui_events::ExploreStorageFailed(error.to_string()));
            }
        }
    }
}
