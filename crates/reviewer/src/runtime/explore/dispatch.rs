use super::ApplicationMessageSender;
use review_explore::{DispatchId, DispatchResult, DispatchState};
use review_store::ReviewStore;
use review_thread_service::{DispatchObserver, PromptError};
use review_types::ReviewUnit;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct DurableDispatch {
    pub(super) store: ReviewStore,
    pub(super) unit: ReviewUnit,
    pub(super) instance: String,
    pub(super) id: DispatchId,
    pub(super) messages: ApplicationMessageSender,
    pub(super) began: AtomicBool,
}

impl DispatchObserver for DurableDispatch {
    fn before_attempt(&self, agent: &herdr_client::protocol::Agent) -> Result<(), String> {
        self.store
            .update_explore(&self.unit, &self.instance, |pass| {
                pass.begin_dispatch(&self.id, agent)
                    .map_err(|e| e.to_string())
            })
            .map(|_| self.began.store(true, Ordering::Release))
            .map_err(|e| e.to_string())
    }

    fn finished(&self, result: &Result<(), PromptError>) -> Result<(), String> {
        let state = Self::outcome(result);
        match self.store.finish_explore_dispatch(
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
                        .messages
                        .send(ui_events::ExploreImplementationSaved(delivery.clone()));
                }
                Ok(())
            }
            Err(error) => {
                let _ = self
                    .messages
                    .send(ui_events::ExploreStorageFailed(error.to_string()));
                Err(error.to_string())
            }
        }
    }
}

impl DurableDispatch {
    pub(super) fn outcome(result: &Result<(), PromptError>) -> DispatchState {
        match result {
            Ok(()) => DispatchState::Delivered,
            Err(PromptError::Cancelled) => DispatchState::Cancelled,
            Err(PromptError::Delivery(error)) => DispatchState::NotSent(error.clone()),
            Err(PromptError::Unknown(_)) => DispatchState::Unknown,
        }
    }
}
