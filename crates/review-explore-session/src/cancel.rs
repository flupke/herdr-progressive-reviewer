//! Cancelling the reviewer's latest answer: its turn's review marks are given
//! back, then the saved pass forgets the answer and the agent's turn after it.

use std::sync::Arc;

use review_explore::ExplorePass;

use crate::ExploreSession;

impl ExploreSession {
    pub(crate) fn cancel_answer(&mut self, answer: String) {
        let result = self
            .cancel_latest(&answer)
            .map_err(|error| error.to_string());
        let _ = self
            .events
            .send(ui_events::ExploreAnswerCancelled { answer, result });
    }

    fn cancel_latest(&mut self, answer: &str) -> eyre::Result<Arc<ExplorePass>> {
        eyre::ensure!(
            self.state.storage_error.is_none(),
            "Explore storage is unavailable: {}",
            self.state.storage_error.as_deref().unwrap_or_default()
        );
        eyre::ensure!(!self.state.historical, "This pass is history");
        let shown = self
            .state
            .pass
            .as_ref()
            .ok_or_else(|| eyre::eyre!("No Explore pass"))?;
        let checkpoint = &shown.exploration.comparison.checkpoint;
        let pass = self
            .passes
            .pass(&checkpoint.review_unit, &shown.exploration.instance)?
            .ok_or_else(|| eyre::eyre!("Saved Explore pass is missing"))?;
        // Refuse against the saved pass before touching review marks.
        let cancelled = pass.clone().cancel_answer(answer)?;
        if let Some(marks) = &cancelled.marks
            && let Some(problem) = self.unmark(&pass, marks)?
        {
            let _ = self.events.send(ui_events::ToastRequested {
                text: format!("Cancelled answer: {problem}"),
                kind: toasts::ToastKind::Error,
            });
        }
        let checkpoint = &pass.exploration.comparison.checkpoint;
        let updated = self.passes.update(
            &checkpoint.review_unit,
            &pass.exploration.instance,
            |pass| {
                pass.cancel_answer(answer)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            },
        );
        let ((), pass) = match updated {
            Ok(updated) => updated,
            Err(error) => {
                // The marks are already given back, by author, so cancelling
                // again after reopening the pass finishes the job.
                self.state.storage_error = Some(error.to_string());
                let _ = self
                    .events
                    .send(ui_events::ExploreStorageFailed(error.to_string()));
                return Err(error.into());
            }
        };
        if let Some((_, request)) = &self.state.pending
            && cancelled.request.as_ref() == Some(request)
        {
            // Withdraw the prompt; the agent's late answer is rejected.
            self.state.prompt = None;
            self.state.pending = None;
            self.state.renew_access();
        }
        self.state.pass = Some(pass.clone());
        Ok(Arc::new(pass))
    }
}
