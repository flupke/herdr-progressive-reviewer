//! Cancelling the reviewer's latest answer: its turn's review marks are given
//! back, then the saved round forgets the answer and the agent's turn after it.

use std::sync::Arc;

use review_explore::ExploreRound;

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

    fn cancel_latest(&mut self, answer: &str) -> eyre::Result<Arc<ExploreRound>> {
        eyre::ensure!(
            self.state.storage_error.is_none(),
            "Explore storage is unavailable: {}",
            self.state.storage_error.as_deref().unwrap_or_default()
        );
        eyre::ensure!(!self.state.historical, "This round is history");
        let shown = self
            .state
            .round
            .as_ref()
            .ok_or_else(|| eyre::eyre!("No Explore round"))?;
        let checkpoint = &shown.exploration.comparison.checkpoint;
        let round = self
            .rounds
            .round(&checkpoint.review_unit, &shown.exploration.instance)?
            .ok_or_else(|| eyre::eyre!("Saved Explore round is missing"))?;
        // Refuse against the saved round before touching review marks.
        let cancelled = round.clone().cancel_answer(answer)?;
        if let Some(marks) = &cancelled.marks
            && let Some(problem) = self.unmark(&round, marks)?
        {
            let _ = self.events.send(ui_events::ToastRequested {
                text: format!("Cancelled answer: {problem}"),
                kind: toasts::ToastKind::Error,
            });
        }
        let checkpoint = &round.exploration.comparison.checkpoint;
        let updated = self.rounds.update(
            &checkpoint.review_unit,
            &round.exploration.instance,
            |round| {
                round
                    .cancel_answer(answer)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            },
        );
        let ((), round) = match updated {
            Ok(updated) => updated,
            // The saved round moved on: nothing is wrong with storage.
            Err(error @ review_store::Error::ExploreRefused(_)) => return Err(error.into()),
            Err(error) => {
                // The marks are already given back, by author, so cancelling
                // again after reopening the round finishes the job.
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
        self.state.round = Some(round.clone());
        Ok(Arc::new(round))
    }
}
