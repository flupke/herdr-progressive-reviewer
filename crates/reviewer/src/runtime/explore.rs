//! Working-copy inputs and MCP submissions; no review-store writes.
use super::{ApplicationMessageSender, Worker, WorkerCommand};
use review_explore::{Command, Comparison, TurnRequest};
use review_thread_service::{PinnedAgent, PromptCancellation};
use std::sync::Arc;
mod implementation;

#[derive(Debug, Default)]
pub(super) struct ExploreRuntime {
    comparison: Option<Arc<Comparison>>,
    pending: Option<(String, String)>,
    agent: Option<PinnedAgent>,
    prompt: Option<PromptCancellation>,
    implementation: Option<PromptCancellation>,
}

impl Worker {
    pub(super) fn explore_command(
        &mut self,
        command: Command,
        messages: &ApplicationMessageSender,
    ) {
        match command {
            Command::Start => self.start_explore(messages),
            Command::Turn(request) => self.explore_turn(*request, messages),
            Command::Implement(request) => self.implement_explore(request, messages),
            Command::CancelImplementation => self.explore.implementation = None,
            Command::Cancel => {
                self.explore.prompt = None;
                self.explore.pending = None;
                self.explore.implementation = None;
            }
        }
    }

    fn start_explore(&mut self, messages: &ApplicationMessageSender) {
        self.explore.prompt = None;
        self.explore.implementation = None;
        self.explore.pending = None;
        let result = self.capture_explore();
        if let Ok(comparison) = &result {
            self.explore.comparison = Some(comparison.clone());
            self.explore.agent = None;
        }
        let _ = messages.send(ui_events::ExploreCaptured {
            result: result.map_err(|error| error.to_string()),
        });
    }

    fn capture_explore(&self) -> eyre::Result<Arc<Comparison>> {
        let review_repository::repository::PollResult::Complete(snapshot) =
            self.repository.poll()?
        else {
            eyre::bail!("Repository comparison is not ready; retry Start");
        };
        Ok(Arc::new(Comparison::prepare(&self.repository, &snapshot)?))
    }

    fn explore_turn(&mut self, request: TurnRequest, messages: &ApplicationMessageSender) {
        self.explore.prompt = None;
        self.explore.implementation = None;
        let result = self.prepare_explore(&request);
        let (prepared, agent) = match result {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = messages.send(ui_events::ExploreFinished {
                    instance: request.instance,
                    request: request.request,
                    result: Err(error.to_string()),
                });
                return;
            }
        };
        let (receipt, cancellation) = self.prompts.send(agent, prepared.prompt());
        self.explore.prompt = Some(cancellation);
        let commands = self.commands.clone();
        std::thread::spawn(move || {
            if let Err(error) = receipt.wait() {
                let _ = commands.send(WorkerCommand::ExploreFinished(Box::new(
                    ui_events::ExploreFinished {
                        instance: request.instance,
                        request: request.request,
                        result: Err(error.to_string()),
                    },
                )));
            }
        });
    }

    fn prepare_explore(
        &mut self,
        request: &TurnRequest,
    ) -> eyre::Result<(review_explore_runner::PreparedTurn, PinnedAgent)> {
        let comparison = self
            .explore
            .comparison
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Start Explore first"))?;
        eyre::ensure!(
            comparison.checkpoint == request.checkpoint,
            "Request does not belong to this comparison"
        );
        let agent = match &self.explore.agent {
            Some(agent) => agent.clone(),
            None => PinnedAgent::new(self.target.resolve(&self.client)?.ok_or_else(|| {
                eyre::eyre!("No implementation agent is available. Select an agent and Retry.")
            })?),
        };
        let prepared = review_explore_runner::PreparedTurn::prepare(request, comparison)?;
        self.explore.pending = Some((request.instance.clone(), request.request.clone()));
        self.explore.agent = Some(agent.clone());
        Ok((prepared, agent))
    }

    pub(super) fn explore_finished(
        &mut self,
        event: ui_events::ExploreFinished,
        messages: &ApplicationMessageSender,
    ) {
        if self.explore.pending.as_ref() != Some(&(event.instance.clone(), event.request.clone())) {
            return;
        }
        self.explore.pending = None;
        self.explore.prompt = None;
        let _ = messages.send(event);
    }

    pub(super) fn explore_mcp(
        &mut self,
        request: review_mcp::Request,
        messages: &ApplicationMessageSender,
    ) {
        if let Err(error) = self.authorize_explore(&request.access) {
            request.respond(Err(error.to_string()));
            return;
        }
        let update = match &request.operation {
            review_mcp::Operation::SubmitQuestion(update)
                if update.next.is_some() && update.conclusion.is_none() =>
            {
                (**update).clone()
            }
            review_mcp::Operation::SubmitConclusion(conclusion) => {
                (**conclusion).clone().into_update()
            }
            _ => {
                request.respond(Err(
                    "submit_question requires one question; use submit_conclusion to finish".into(),
                ));
                return;
            }
        };
        if update.instance != request.access {
            request.respond(Err("Explore response belongs to another instance".into()));
            return;
        }
        let (response, received) = std::sync::mpsc::channel();
        if messages
            .send(ui_events::ExploreSubmission { update, response })
            .is_err()
        {
            request.respond(Err("The reviewer is closed".into()));
            return;
        }
        std::thread::spawn(move || {
            let result = received
                .recv_timeout(std::time::Duration::from_secs(15))
                .map_err(|_| {
                    "The reviewer did not acknowledge Explore; retry the identical payload"
                        .to_owned()
                })
                .and_then(|result| result)
                .map(|applied| review_mcp::Response::Explore { applied });
            request.respond(result);
        });
    }

    fn authorize_explore(&mut self, access: &str) -> eyre::Result<()> {
        eyre::ensure!(
            self.explore
                .pending
                .as_ref()
                .is_some_and(|(instance, _)| instance == access),
            "Unknown or cancelled Explore access value; use the active request.instance"
        );
        self.explore
            .agent
            .as_ref()
            .ok_or_else(|| eyre::eyre!("No interview agent"))?
            .current(&self.client)
            .map_err(eyre::Report::msg)?
            .ok_or_else(|| {
                eyre::eyre!("Waiting for Herdr to identify the interview agent's session")
            })?;
        Ok(())
    }
}
