//! Explicitly authorized implementation of an Explore conclusion.

use crate::{ExploreSession, dispatch::DurableDispatch, turn_log::SentTurn};
use review_explore::{DispatchState, ImplementationRequest};
use std::sync::Arc;
use ui_events::ExploreImplementationFinished;

impl ExploreSession {
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
                    state: DispatchState::NotSent(error.clone()),
                });
                return Err(error);
            }
        };
        let _ = self.events.send(ui_events::ExploreImplementationSaved(
            round.implementations[&request.delivery].clone(),
        ));
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
        self.state.implementation = Some(cancellation);
        let events = self.events.clone();
        let attempt = round.implementations[&request.delivery].attempt.clone();
        std::thread::spawn(move || {
            let _ = events.send(ExploreImplementationFinished {
                request,
                attempt: Some(attempt),
                state: DurableDispatch::outcome(&receipt.wait()),
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
