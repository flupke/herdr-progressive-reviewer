use crate::records::SavedRounds;
use crate::turn_log::{SentTurn, TurnLog};
use component_core::ApplicationEventSender;
use review_explore::{DispatchId, DispatchResult, DispatchState};
use review_thread_service::{DispatchObserver, PromptError};
use review_types::ReviewUnit;
use std::sync::atomic::{AtomicBool, Ordering};

/// Records each prompt attempt in the saved round before and after delivery.
pub(crate) struct DurableDispatch {
    pub(crate) rounds: SavedRounds,
    pub(crate) unit: ReviewUnit,
    pub(crate) instance: String,
    pub(crate) id: DispatchId,
    pub(crate) events: ApplicationEventSender,
    pub(crate) began: AtomicBool,
    /// The prompt, for a vision session's record of what was sent.
    pub(crate) turn: Option<(TurnLog, SentTurn)>,
}

impl DispatchObserver for DurableDispatch {
    fn before_attempt(&self, agent: &herdr_client::protocol::Agent) -> Result<(), String> {
        self.rounds
            .update(&self.unit, &self.instance, |round| {
                round
                    .begin_dispatch(&self.id, agent)
                    .map_err(|e| e.to_string())
            })
            .map(|_| self.began.store(true, Ordering::Release))
            .map_err(|e| e.to_string())
    }

    fn finished(&self, result: &Result<(), PromptError>) -> Result<(), String> {
        let state = Self::outcome(result);
        let saved = self.rounds.finish_dispatch(
            &self.unit,
            &self.instance,
            &DispatchResult {
                id: self.id.clone(),
                began: self.began.load(Ordering::Acquire),
                state,
            },
        );
        // Recorded after the save, so a reply never outruns the saved dispatch.
        if let Some((turns, sent)) = &self.turn {
            match &saved {
                Ok(_) => turns.record(sent, result),
                Err(error) => turns.record(sent, &Err(PromptError::Unknown(error.to_string()))),
            }
        }
        match saved {
            Ok(round) => {
                if let DispatchId::Implementation { request, .. } = &self.id
                    && let Some(delivery) = round.implementations.get(request)
                {
                    let _ = self
                        .events
                        .send(ui_events::ExploreImplementationSaved(delivery.clone()));
                }
                Ok(())
            }
            Err(error) => {
                let _ = self
                    .events
                    .send(ui_events::ExploreStorageFailed(error.to_string()));
                Err(error.to_string())
            }
        }
    }
}

impl DurableDispatch {
    pub(crate) fn outcome(result: &Result<(), PromptError>) -> DispatchState {
        match result {
            Ok(()) => DispatchState::Delivered,
            Err(PromptError::Cancelled) => DispatchState::Cancelled,
            Err(PromptError::NotStarted) => DispatchState::NotStarted,
            Err(PromptError::Delivery(error)) => DispatchState::NotSent(error.clone()),
            Err(PromptError::Unknown(_)) => DispatchState::Unknown,
        }
    }
}
