//! Confirmed bulk review-mark changes and their UI updates.

use super::{ApplicationEventSender, ReviewCheckpoint, ReviewStateSaved, Worker, WorkerCommand};

impl Worker {
    pub(super) fn handle_review_command(
        &mut self,
        command: WorkerCommand,
        messages: &ApplicationEventSender,
    ) {
        match command {
            WorkerCommand::SetReviewed { path, reviewed } => {
                self.cancel_auto_review();
                self.set_reviewed(messages, path, reviewed);
            }
            WorkerCommand::AutoReview(checkpoint) => self.start_auto_review(&checkpoint, messages),
            WorkerCommand::AutoReviewFinished(review) => self.finish_auto_review(&review, messages),
            WorkerCommand::UnreviewAll(checkpoint) => self.unreview_all(&checkpoint, messages),
            _ => unreachable!("review commands are routed by the worker"),
        }
    }

    fn unreview_all(&mut self, checkpoint: &ReviewCheckpoint, messages: &ApplicationEventSender) {
        let result = self.clear_review_marks(checkpoint);
        // Even a partial storage failure must be reflected in Files and loaded diffs.
        self.poll(messages);
        if let Some(snapshot) = &self.snapshot {
            for file in &snapshot.files {
                let _ = messages.send(ReviewStateSaved {
                    review_unit: snapshot.identity.review_unit().clone(),
                    path: file.review_path().display(),
                    result: self.tracker.status(snapshot, file).map_err(|_| ()),
                });
            }
        }
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
