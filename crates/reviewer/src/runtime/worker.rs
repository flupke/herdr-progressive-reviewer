//! Repository work in arrival order: refreshes, revision edits, review marks and Explore.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender};

use component_core::ApplicationEventSender;
use review_explore_session::{self as explore_session, ExploreSession};
use review_hunks::HunkMark;
use review_repository::repository::{ChangeId, ChangedFile, PollResult, Repository, Snapshot};
use review_significance::JevClassifier;
use review_source::ReviewCheckpoint;
use review_state::{MarkResult, ReviewState, ReviewTracker};
use review_store::ReviewStore;
use review_types::MarkAuthor;
use review_ui::RepositoryAction;
use ui_events::{
    FileSummary, RepositoryFilesChanged, RepositoryMetadataChanged, RepositoryRefreshFinished,
    ReviewStateSaved, RevisionCandidatesLoaded, RevisionEditFailed, RevisionHistoryLoaded,
};

use super::{auto_review, document};

#[derive(Debug)]
pub(super) struct Worker {
    pub(super) repository: Repository,
    pub(super) tracker: Arc<ReviewTracker>,
    pub(super) store: ReviewStore,
    pub(super) snapshot: Option<Snapshot>,
    pub(super) commands: Sender<WorkerCommand>,
    pub(super) explore: ExploreSession,
    pub(super) jev: JevClassifier,
    pub(super) auto_review: Option<Arc<AtomicBool>>,
    /// An Explore kickoff waiting for Jev to mark what it dismisses first.
    pub(super) held_kickoff: Option<review_explore::TurnRequest>,
    pub(super) documents: Sender<document::Command>,
}

#[derive(Debug)]
pub(super) enum WorkerCommand {
    Explore(explore_session::Input),
    Repository(RepositoryAction),
    Poll,
    AutoReviewFinished(Box<auto_review::AutoReview>),
    /// Block until the sender drops, keeping later work pending.
    #[cfg(test)]
    Hold(crossbeam_channel::Receiver<()>),
    Quit,
}

impl Worker {
    pub(super) fn run(
        &mut self,
        commands: &Receiver<WorkerCommand>,
        messages: &ApplicationEventSender,
    ) {
        let mut next = None;
        while let Some(mut command) = next.take().or_else(|| commands.recv().ok()) {
            if let WorkerCommand::Explore(explore_session::Input::Command(
                review_explore::Command::SaveView(view),
            )) = &mut command
            {
                for _ in 0..64 {
                    match commands.try_recv() {
                        Ok(WorkerCommand::Explore(explore_session::Input::Command(
                            review_explore::Command::SaveView(new),
                        ))) if new.instance == view.instance => {
                            *view = new;
                        }
                        Ok(command) => {
                            next = Some(command);
                            break;
                        }
                        Err(_) => break,
                    }
                }
            }
            if !self.handle_command(command, messages) {
                return;
            }
        }
    }

    fn handle_command(
        &mut self,
        command: WorkerCommand,
        messages: &ApplicationEventSender,
    ) -> bool {
        match command {
            WorkerCommand::Poll => {
                let _ = self.poll(messages);
                let _ = messages.send(RepositoryRefreshFinished);
            }
            WorkerCommand::Repository(action) => self.handle_repository_action(action, messages),
            WorkerCommand::AutoReviewFinished(review) => self.finish_auto_review(&review, messages),
            WorkerCommand::Explore(explore_session::Input::Command(
                review_explore::Command::Turn(request),
            )) if request.answer.is_none() => self.start_round(*request, messages),
            WorkerCommand::Explore(input) => {
                if matches!(
                    input,
                    explore_session::Input::Command(
                        review_explore::Command::Start
                            | review_explore::Command::Reset
                            | review_explore::Command::Cancel
                    )
                ) {
                    self.held_kickoff = None;
                }
                self.explore.handle(input);
            }
            #[cfg(test)]
            WorkerCommand::Hold(release) => {
                let _ = release.recv();
            }
            WorkerCommand::Quit => return false,
        }
        true
    }

    fn handle_repository_action(
        &mut self,
        action: RepositoryAction,
        messages: &ApplicationEventSender,
    ) {
        match action {
            RepositoryAction::LoadRevisionCandidates(direction) => {
                let result = self
                    .repository
                    .revision_candidates(direction)
                    .map_err(|error| error.to_string());
                let _ = messages.send(RevisionCandidatesLoaded { direction, result });
            }
            RepositoryAction::LoadRevisionHistory { load_id } => {
                let result = self
                    .repository
                    .revision_history()
                    .map_err(|error| error.to_string());
                let _ = messages.send(RevisionHistoryLoaded { load_id, result });
            }
            RepositoryAction::EditRevision { change_id } => {
                self.edit_revision(messages, &change_id);
            }
            RepositoryAction::SetReviewed { path, reviewed } => {
                self.cancel_auto_review();
                self.set_reviewed(messages, path, reviewed);
            }
            RepositoryAction::SetHunkReviewed {
                review_checkpoint,
                path,
                mark,
            } => {
                self.cancel_auto_review();
                self.set_hunk_reviewed(messages, &review_checkpoint, path, &mark);
            }
            RepositoryAction::AutoReview(checkpoint) => {
                self.start_auto_review(&checkpoint, messages);
            }
            RepositoryAction::UnreviewAll(checkpoint) => self.unreview_all(&checkpoint, messages),
        }
    }

    fn edit_revision(&mut self, messages: &ApplicationEventSender, change_id: &ChangeId) {
        let failure = match self.repository.edit_revision(change_id) {
            Ok(true) if !self.poll(messages) => {
                Some("could not load the selected revision".to_owned())
            }
            Ok(true) => return,
            Ok(false) => Some("selected revision is immutable or unavailable".to_owned()),
            Err(error) => Some(error.to_string()),
        };
        let _ = messages.send(RevisionEditFailed { message: failure });
    }

    /// Publish the current comparison: documents get the snapshot before the
    /// application hears of its files, and Explore restores the review's round last.
    pub(super) fn poll(&mut self, messages: &ApplicationEventSender) -> bool {
        let snapshot = match self.repository.poll() {
            Ok(PollResult::Complete(snapshot)) => snapshot,
            Ok(PollResult::ChangedDuringPoll) | Err(_) => return false,
        };
        let Ok(states) = self.tracker.statuses(&snapshot) else {
            return false;
        };
        let files = snapshot
            .files
            .iter()
            .zip(&states)
            .map(|(file, state)| FileSummary::from_review_state(file, *state))
            .collect();
        let review_checkpoint = ReviewCheckpoint::new(
            snapshot.identity.review_unit().clone(),
            snapshot.identity.snapshot_id(),
        );
        if self
            .snapshot
            .as_ref()
            .is_some_and(|previous| previous.identity != snapshot.identity)
        {
            self.cancel_auto_review();
        }
        let _ = self
            .documents
            .send(document::Command::Snapshot(snapshot.clone()));
        let _ = messages.send(RepositoryMetadataChanged {
            review_checkpoint: review_checkpoint.clone(),
            description: snapshot.identity.description().to_owned(),
            display_id: snapshot.identity.display_id().to_owned(),
        });
        let _ = messages.send(RepositoryFilesChanged {
            review_checkpoint,
            files,
        });
        let review_unit = snapshot.identity.review_unit().clone();
        self.explore.checkpoint_changed(&review_unit);
        self.snapshot = Some(snapshot);
        true
    }

    fn set_hunk_reviewed(
        &self,
        messages: &ApplicationEventSender,
        review_checkpoint: &ReviewCheckpoint,
        path: String,
        mark: &HunkMark,
    ) {
        // Always answer: the diff waits for this result before the next mark.
        let result = (|| {
            let snapshot = self
                .snapshot
                .as_ref()
                .filter(|snapshot| {
                    review_checkpoint.matches(
                        snapshot.identity.review_unit(),
                        snapshot.identity.snapshot_id(),
                    )
                })
                .ok_or_else(|| eyre::eyre!("the change moved; wait for the next refresh"))?;
            let file = changed_file(snapshot, &path)?;
            let marked = self
                .tracker
                .mark_hunk(snapshot, file, mark, &MarkAuthor::Reviewer)?;
            self.marked_state(snapshot, file, marked)
        })();
        if let Err(error) = &result {
            let _ = messages.send(ui_events::ToastRequested {
                text: format!("Could not change the hunk review mark: {error}"),
                kind: toasts::ToastKind::Error,
            });
        }
        let _ = messages.send(ReviewStateSaved {
            review_unit: review_checkpoint.review_unit.clone(),
            path,
            result: result.map_err(|_| ()),
        });
    }

    fn set_reviewed(&self, messages: &ApplicationEventSender, path: String, reviewed: bool) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let result = changed_file(snapshot, &path)
            .and_then(|file| {
                if reviewed {
                    let marked = self.tracker.mark(snapshot, file, &MarkAuthor::Reviewer)?;
                    self.marked_state(snapshot, file, marked)
                } else {
                    self.tracker.unreview(snapshot, file)?;
                    self.tracker.status(snapshot, file)
                }
            })
            .map_err(|_| ());
        let _ = messages.send(ReviewStateSaved {
            review_unit: snapshot.identity.review_unit().clone(),
            path,
            result,
        });
    }

    /// The state a stored mark left, or why it was not stored.
    fn marked_state(
        &self,
        snapshot: &Snapshot,
        file: &ChangedFile,
        marked: MarkResult,
    ) -> eyre::Result<ReviewState> {
        match marked {
            MarkResult::Marked | MarkResult::NothingToMark => self.tracker.status(snapshot, file),
            MarkResult::ChangeChanged => {
                eyre::bail!("the change moved; wait for the next refresh");
            }
        }
    }
}

/// The changed file at `path` in the worker's snapshot.
fn changed_file<'a>(snapshot: &'a Snapshot, path: &str) -> eyre::Result<&'a ChangedFile> {
    snapshot
        .files
        .iter()
        .find(|file| file.review_path().display() == path)
        .ok_or_else(|| eyre::eyre!("the selected file is no longer in the current change"))
}

#[cfg(test)]
#[path = "worker.tests.rs"]
mod tests;
