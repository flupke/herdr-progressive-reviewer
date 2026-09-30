use crate::records::SavedPasses;
use component_core::ApplicationEventSender;
use review_explore::{DispatchId, DispatchResult, DispatchState};
use review_thread_service::{DispatchObserver, PromptError};
use review_types::ReviewUnit;
use std::sync::atomic::{AtomicBool, Ordering};

/// Records each prompt attempt in the saved pass before and after delivery.
pub(crate) struct DurableDispatch {
    pub(crate) passes: SavedPasses,
    pub(crate) unit: ReviewUnit,
    pub(crate) instance: String,
    pub(crate) id: DispatchId,
    pub(crate) events: ApplicationEventSender,
    pub(crate) began: AtomicBool,
}

impl DispatchObserver for DurableDispatch {
    fn before_attempt(&self, agent: &herdr_client::protocol::Agent) -> Result<(), String> {
        self.passes
            .update(&self.unit, &self.instance, |pass| {
                pass.begin_dispatch(&self.id, agent)
                    .map_err(|e| e.to_string())
            })
            .map(|_| self.began.store(true, Ordering::Release))
            .map_err(|e| e.to_string())
    }

    fn finished(&self, result: &Result<(), PromptError>) -> Result<(), String> {
        let state = Self::outcome(result);
        match self.passes.finish_dispatch(
            &self.unit,
            &self.instance,
            &DispatchResult {
                id: self.id.clone(),
                began: self.began.load(Ordering::Acquire),
                state,
            },
        ) {
            Ok(pass) => {
                if let DispatchId::Implementation { request, .. } = &self.id
                    && let Some(delivery) = pass.implementations.get(request)
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
            Err(PromptError::Delivery(error)) => DispatchState::NotSent(error.clone()),
            Err(PromptError::Unknown(_)) => DispatchState::Unknown,
        }
    }
}
