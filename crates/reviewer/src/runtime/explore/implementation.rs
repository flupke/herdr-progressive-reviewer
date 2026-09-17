use super::{ApplicationMessageSender, Worker};
use review_explore::ImplementationRequest;
use ui_events::ExploreImplementationFinished;

impl Worker {
    pub(super) fn implement_explore(
        &mut self,
        request: ImplementationRequest,
        messages: &ApplicationMessageSender,
    ) {
        let result = self.prepare_implementation(&request);
        let agent = match result {
            Ok(agent) => agent,
            Err(error) => {
                let _ = messages.send(ExploreImplementationFinished {
                    request,
                    result: Err(error.to_string()),
                });
                return;
            }
        };
        let prompt = review_explore_runner::implementation_prompt(&request);
        let (receipt, cancellation) = self.prompts.send(agent, prompt);
        self.explore.implementation = Some(cancellation);
        let messages = messages.clone();
        std::thread::spawn(move || {
            let _ = messages.send(ExploreImplementationFinished {
                request,
                result: receipt.wait().map_err(|error| error.to_string()),
            });
        });
    }

    fn prepare_implementation(
        &mut self,
        request: &ImplementationRequest,
    ) -> eyre::Result<review_thread_service::PinnedAgent> {
        self.authorize_explore(&request.instance)?;
        eyre::ensure!(
            self.explore.pending.as_ref()
                == Some(&(request.instance.clone(), request.conclusion.clone())),
            "This conclusion has been superseded"
        );
        eyre::ensure!(
            !request.text.trim().is_empty(),
            "Add implementation tasks first"
        );
        Ok(self
            .explore
            .agent
            .clone()
            .expect("authorized interview agent"))
    }
}
