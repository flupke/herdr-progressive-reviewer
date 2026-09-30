//! Explicitly authorized implementation of an Explore conclusion.

use crate::{ExploreSession, dispatch::DurableDispatch};
use review_explore::ImplementationRequest;
use std::sync::Arc;
use ui_events::ExploreImplementationFinished;

impl ExploreSession {
    pub(crate) fn implement(&mut self, request: ImplementationRequest) {
        let result = self.prepare_implementation(&request);
        let (agent, pass) = match result {
            Ok(result) => result,
            Err(error) => {
                let _ = self.events.send(ExploreImplementationFinished {
                    request,
                    attempt: None,
                    state: review_explore::DispatchState::NotSent(error.to_string()),
                });
                return;
            }
        };
        let _ = self.events.send(ui_events::ExploreImplementationSaved(
            pass.implementations[&request.delivery].clone(),
        ));
        let observer = DurableDispatch {
            began: std::sync::atomic::AtomicBool::default(),
            passes: self.passes.clone(),
            unit: pass.exploration.comparison.checkpoint.review_unit.clone(),
            instance: request.instance.clone(),
            id: review_explore::DispatchId::Implementation {
                request: request.delivery.clone(),
                attempt: pass.implementations[&request.delivery].attempt.clone(),
            },
            events: self.events.clone(),
        };
        let prompt = review_explore_runner::implementation_prompt(&request);
        let (receipt, cancellation) =
            self.prompts
                .send_observed(agent, prompt, Some(Arc::new(observer)));
        self.state.implementation = Some(cancellation);
        let events = self.events.clone();
        let attempt = pass.implementations[&request.delivery].attempt.clone();
        std::thread::spawn(move || {
            let _ = events.send(ExploreImplementationFinished {
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
            self.state.storage_error.is_none() && !self.state.historical,
            "Explore storage is unavailable or this pass is history"
        );
        let agent = self.select_agent()?;
        let unit = self
            .state
            .loaded_unit
            .clone()
            .ok_or_else(|| eyre::eyre!("No Explore pass"))?;
        let ((), pass) = self.passes.update(&unit, &request.instance, |pass| {
            pass.authorize(request).map_err(|error| error.to_string())?;
            pass.implementations
                .get_mut(&request.delivery)
                .expect("authorized")
                .state = review_explore::DispatchState::Queued;
            Ok(())
        })?;
        self.state.pass = Some(pass.clone());
        Ok((agent, pass))
    }
}
