//! Cancelling one of the reviewer's answers with every answer after it: the review marks they
//! applied are given back, then the saved round forgets the answers and the agent's turns after
//! them.

use std::sync::Arc;

use review_explore::ExploreRound;

use crate::ExploreSession;

/// Which answers Cancel answer may take back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Cancel {
    /// The latest answer only, as the pane offers it: the pane may not have heard of a later
    /// answer yet.
    Latest,
    /// Any answer, with every answer after it, as an earlier question on the Explore page
    /// offers it.
    From,
}

impl ExploreSession {
    /// Cancels the answer `answer`, as `cancel` allows, with every answer after it, tells the
    /// front ends, and returns why it could not, when it could not.
    pub(crate) fn cancel_answer(&mut self, answer: String, cancel: Cancel) -> Result<(), String> {
        let result = self
            .cancel_from(&answer, cancel)
            .map_err(|error| error.to_string());
        if result.is_ok() {
            self.run_ahead_discard(review_run_ahead::DiscardReason::AnswerCancelled);
        }
        let outcome = result.as_ref().map(|_| ()).map_err(Clone::clone);
        let _ = self
            .events
            .send(ui_events::ExploreAnswerCancelled { answer, result });
        outcome
    }

    fn cancel_from(&mut self, answer: &str, cancel: Cancel) -> eyre::Result<Arc<ExploreRound>> {
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
        eyre::ensure!(
            cancel == Cancel::From
                || round
                    .exploration
                    .answers
                    .last()
                    .is_some_and(|latest| latest.id == answer),
            "Only the latest answer can be cancelled"
        );
        let cancelled = round.clone().cancel_answers_from(answer)?;
        let mut problems = std::collections::BTreeSet::new();
        for cancelled in &cancelled {
            for marks in &cancelled.marks {
                problems.extend(self.unmark(&round, &cancelled.answer.id, marks)?);
            }
        }
        for problem in problems {
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
                    .cancel_answers_from(answer)
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
            && cancelled
                .iter()
                .any(|cancelled| cancelled.request.as_ref() == Some(request))
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
