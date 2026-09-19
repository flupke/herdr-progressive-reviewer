use super::{ApplicationMessageSender, Worker, dispatch::DurableDispatch};
use review_explore::ImplementationRequest;
use std::sync::Arc;
use ui_events::ExploreImplementationFinished;

impl Worker {
    pub(super) fn implement_explore(
        &mut self,
        request: ImplementationRequest,
        messages: &ApplicationMessageSender,
    ) {
        let result = self.prepare_implementation(&request);
        let (agent, pass) = match result {
            Ok(result) => result,
            Err(error) => {
                let _ = messages.send(ExploreImplementationFinished {
                    request,
                    attempt: None,
                    state: review_explore::DispatchState::NotSent(error.to_string()),
                });
                return;
            }
        };
        let _ = messages.send(ui_events::ExploreImplementationSaved(
            pass.implementations[&request.delivery].clone(),
        ));
        let observer = DurableDispatch {
            began: std::sync::atomic::AtomicBool::default(),
            store: self.guide_store.clone(),
            unit: pass.exploration.comparison.checkpoint.review_unit.clone(),
            instance: request.instance.clone(),
            id: review_explore::DispatchId::Implementation {
                request: request.delivery.clone(),
                attempt: pass.implementations[&request.delivery].attempt.clone(),
            },
            messages: messages.clone(),
        };
        let prompt = review_explore_runner::implementation_prompt(&request);
        let (receipt, cancellation) =
            self.prompts
                .send_observed(agent, prompt, Some(Arc::new(observer)));
        self.explore.implementation = Some(cancellation);
        let messages = messages.clone();
        let attempt = pass.implementations[&request.delivery].attempt.clone();
        std::thread::spawn(move || {
            let _ = messages.send(ExploreImplementationFinished {
                request,
                attempt: Some(attempt),
                state: DurableDispatch::outcome(&receipt.wait()),
            });
        });
    }

    fn prepare_implementation(
        &mut self,
        request: &ImplementationRequest,
    ) -> eyre::Result<(
        review_thread_service::PinnedAgent,
        review_explore::ExplorePass,
    )> {
        eyre::ensure!(
            self.explore.storage_error.is_none() && !self.explore.historical,
            "Explore storage is unavailable or this pass is history"
        );
        let agent = self.bound_explore_agent()?;
        let unit = self
            .explore
            .loaded_unit
            .clone()
            .ok_or_else(|| eyre::eyre!("No Explore pass"))?;
        let current = agent
            .current(&self.client)
            .map_err(eyre::Report::msg)?
            .filter(|agent| agent.agent_session.is_some())
            .ok_or_else(|| eyre::eyre!("Waiting for the original native conversation"))?;
        let ((), pass) = self
            .guide_store
            .update_explore(&unit, &request.instance, |pass| {
                if !pass
                    .binding
                    .as_ref()
                    .is_some_and(|known| known.matches(&current))
                {
                    return Err("Original native conversation does not match".into());
                }
                pass.authorize(request).map_err(|error| error.to_string())?;
                pass.implementations
                    .get_mut(&request.delivery)
                    .expect("authorized")
                    .state = review_explore::DispatchState::Queued;
                Ok(())
            })?;
        self.explore.pass = Some(pass.clone());
        Ok((agent, pass))
    }
}
