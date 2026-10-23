//! Explicitly authorized implementation of an Explore conclusion.

use crate::{ExploreSession, dispatch::DurableDispatch, turn_log::SentTurn};
use review_explore::{
    DispatchState, ExploreRound, ImplementationDelivery, ImplementationRequest, ImplementationState,
};
use std::collections::BTreeSet;
use std::sync::Arc;
use ui_events::{ExploreImplementationFinished, ImplementationProgress};

/// What the front ends show of `delivery`, which this process sends when `sending`.
pub(crate) fn progress(delivery: &ImplementationDelivery, sending: bool) -> ImplementationProgress {
    ImplementationProgress {
        request: delivery.request.clone(),
        attempt: delivery.attempt.clone(),
        state: ImplementationState::of(&delivery.state, sending),
    }
}

/// The implementation request this process sends; dropping it cancels its prompt.
#[derive(Debug)]
pub(crate) struct SendingImplementation {
    /// The request's delivery ID.
    delivery: String,
    _cancellation: review_thread_service::PromptCancellation,
}

/// The latest implementation request of each conclusion of `round`, with the one whose
/// delivery ID is `sending` sent by this process.
pub(crate) fn round_progress(
    round: &ExploreRound,
    sending: Option<&str>,
) -> Vec<ImplementationProgress> {
    let conclusions: BTreeSet<_> = round
        .implementations
        .values()
        .map(|delivery| delivery.request.conclusion.as_str())
        .collect();
    conclusions
        .into_iter()
        .filter_map(|conclusion| round.latest_implementation(conclusion))
        .map(|delivery| {
            progress(
                delivery,
                sending == Some(delivery.request.delivery.as_str()),
            )
        })
        .collect()
}

impl ExploreSession {
    /// Publish a durable round change nobody waits to acknowledge; false once the application
    /// stopped receiving events.
    pub(crate) fn publish_committed(&self, round: ExploreRound) -> bool {
        let (response, _) = std::sync::mpsc::channel();
        self.events
            .send(ui_events::ExploreCommitted {
                implementations: self.implementation_progress(&round),
                round: Arc::new(round),
                applied: true,
                response,
            })
            .is_ok()
    }

    /// The latest implementation request of each conclusion of `round`, as this process shows
    /// it now.
    pub(crate) fn implementation_progress(
        &self,
        round: &ExploreRound,
    ) -> Vec<ImplementationProgress> {
        let sending = self
            .state
            .implementation
            .as_ref()
            .map(|sending| sending.delivery.as_str());
        round_progress(round, sending)
    }

    /// Saves the authorized `request`, then prompts the agent with it, and tells the front
    /// ends. Errs, with the reason, only when the request was not saved: a prompt that fails
    /// after the save leaves the request saved as not sent.
    pub(crate) fn implement(&mut self, request: ImplementationRequest) -> Result<(), String> {
        let result = self.prepare_implementation(&request);
        let (agent, round) = match result {
            Ok(result) => result,
            Err(error) => {
                let error = error.to_string();
                let _ = self.events.send(ExploreImplementationFinished {
                    request,
                    attempt: None,
                    state: ImplementationState::NotSent(error.clone()),
                });
                return Err(error);
            }
        };
        let _ = self
            .events
            .send(ui_events::ExploreImplementationSaved(progress(
                &round.implementations[&request.delivery],
                true,
            )));
        let prompt =
            review_explore_runner::implementation_prompt(&request, round.exploration.author);
        let observer = DurableDispatch {
            began: std::sync::atomic::AtomicBool::default(),
            rounds: self.rounds.clone(),
            unit: round.exploration.comparison.checkpoint.review_unit.clone(),
            instance: request.instance.clone(),
            id: review_explore::DispatchId::Implementation {
                request: request.delivery.clone(),
                attempt: round.implementations[&request.delivery].attempt.clone(),
            },
            events: self.events.clone(),
            turn: self
                .turns
                .clone()
                .map(|turns| (turns, SentTurn::implement(prompt.clone()))),
        };
        let (receipt, cancellation) =
            self.prompts
                .send_observed(agent, prompt, Some(Arc::new(observer)));
        self.state.implementation = Some(SendingImplementation {
            delivery: request.delivery.clone(),
            _cancellation: cancellation,
        });
        let events = self.events.clone();
        let attempt = round.implementations[&request.delivery].attempt.clone();
        std::thread::spawn(move || {
            let _ = events.send(ExploreImplementationFinished {
                request,
                attempt: Some(attempt),
                state: ImplementationState::of(&DurableDispatch::outcome(&receipt.wait()), true),
            });
        });
        Ok(())
    }

    fn prepare_implementation(
        &mut self,
        request: &ImplementationRequest,
    ) -> eyre::Result<(
        review_thread_service::PinnedAgent,
        review_explore::ExploreRound,
    )> {
        eyre::ensure!(
            self.state.storage_error.is_none() && !self.state.historical,
            "Explore storage is unavailable or this round is history"
        );
        let agent = self.select_agent()?;
        let unit = self
            .state
            .loaded_unit
            .clone()
            .ok_or_else(|| eyre::eyre!("No Explore round"))?;
        let sending = self.state.implementation.is_some();
        // Refuse against the saved round before touching review marks.
        let saved = self
            .rounds
            .round(&unit, &request.instance)?
            .ok_or_else(|| eyre::eyre!("Saved Explore round is missing"))?;
        admit(&mut saved.clone(), request, sending).map_err(|error| eyre::eyre!(error))?;
        // At the drawing board, the code is about to change for another design: what the round's
        // answers settled goes back to review, for Implement as for Post review. Only now, since
        // Cancel answer, which would have to mark the lines again, is offered until a request is
        // made; and before the request is saved, so that a failure refuses Implement, which the
        // reviewer tries again.
        if saved.exploration.concluded_at_drawing_board() {
            self.reopen_round_marks(&saved)?;
        }
        let ((), round) = self.rounds.update(&unit, &request.instance, |round| {
            admit(round, request, sending)
        })?;
        self.state.round = Some(round.clone());
        Ok((agent, round))
    }
}

/// Authorizes `request` in `round`, queued, unless another request holds its conclusion: one this
/// process sends (`sending`), or one the agent received, since a second front end that did not see
/// it yet must not start another implementation. A request an earlier process left paused or
/// unknown leaves the reviewer the choice.
fn admit(
    round: &mut review_explore::ExploreRound,
    request: &ImplementationRequest,
    sending: bool,
) -> Result<(), String> {
    let held = round
        .latest_implementation(&request.conclusion)
        .filter(|latest| latest.request.delivery != request.delivery)
        .is_some_and(|latest| match latest.state {
            DispatchState::Delivered => true,
            DispatchState::Queued | DispatchState::Attempting => sending,
            _ => false,
        });
    if held {
        return Err("An implementation request for this conclusion was sent already".into());
    }
    round
        .authorize(request)
        .map_err(|error| error.to_string())?;
    round
        .implementations
        .get_mut(&request.delivery)
        .expect("authorized")
        .state = DispatchState::Queued;
    Ok(())
}
