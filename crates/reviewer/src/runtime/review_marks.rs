//! Confirmed bulk review-mark changes and their UI updates.

use super::worker::Worker;
use component_core::ApplicationEventSender;
use review_repository::repository::ChangedFile;
use review_source::ReviewCheckpoint;
use ui_events::ReviewStateSaved;

impl Worker {
    pub(super) fn unreview_all(
        &mut self,
        checkpoint: &ReviewCheckpoint,
        messages: &ApplicationEventSender,
    ) {
        let result = self.clear_review_marks(checkpoint);
        // Even a partial storage failure must be reflected in Files and loaded diffs.
        self.poll(messages);
        self.announce_review_states(messages, |_| true);
        let toast = match result {
            Ok(()) => ui_events::ToastRequested {
                text: "All files set to unreviewed.".into(),
                kind: toasts::ToastKind::Info,
            },
            Err(error) => ui_events::ToastRequested {
                text: format!("Could not reset file review marks: {error}"),
                kind: toasts::ToastKind::Error,
            },
        };
        let _ = messages.send(toast);
    }

    /// Tell Files and loaded diffs the current review state of the chosen
    /// files: marks changed without a new comparison reload nothing else.
    pub(super) fn announce_review_states(
        &self,
        messages: &ApplicationEventSender,
        chosen: impl Fn(&ChangedFile) -> bool,
    ) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        for file in snapshot.files.iter().filter(|file| chosen(file)) {
            let _ = messages.send(ReviewStateSaved {
                review_unit: snapshot.identity.review_unit().clone(),
                path: file.review_path().display(),
                result: self.tracker.status(snapshot, file).map_err(|_| ()),
            });
        }
    }

    fn clear_review_marks(&self, checkpoint: &ReviewCheckpoint) -> eyre::Result<()> {
        let current = self.repository.current_identity()?;
        eyre::ensure!(
            current.review_unit() == &checkpoint.review_unit
                && current.snapshot_id() == checkpoint.checkpoint,
            "The comparison changed; retry rU after refresh"
        );
        self.cancel_auto_review();
        self.store.unreview_all(&checkpoint.review_unit)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
