//! Interview turns: persist the reviewer's request, then prompt the pinned agent.

use std::sync::Arc;

use herdr_client::protocol::Agent;
use review_explore::{ConversationBinding, Exploration, ExplorePass, TurnRequest};
use review_thread_service::PinnedAgent;

use crate::{ExploreSession, Input, dispatch::DurableDispatch};

impl ExploreSession {
    pub(crate) fn retry(&mut self, request: TurnRequest) {
        let retry = match self.retry_agent().and_then(|agent| {
            let pinned =
                PinnedAgent::for_retry(agent.clone(), &*self.agents).map_err(eyre::Report::msg)?;
            Ok((agent, pinned))
        }) {
            Ok(retry) => retry,
            Err(error) => {
                let _ = self.events.send(ui_events::ExplorePosted {
                    request,
                    result: Err(error.to_string()),
                });
                return;
            }
        };
        self.deliver_turn(request, Some((&retry.0, retry.1)));
    }

    pub(crate) fn deliver_turn(
        &mut self,
        request: TurnRequest,
        retry_agent: Option<(&Agent, PinnedAgent)>,
    ) {
        self.state.prompt = None;
        self.state.implementation = None;
        // Preserve the posted contribution even when its subsequent wakeup cannot be sent.
        if retry_agent.is_none() {
            let _ = self.select_agent();
        }
        let persisted =
            self.persist_request(&request, retry_agent.as_ref().map(|(agent, _)| *agent));
        let pass = match persisted {
            Ok(pass) => pass,
            Err(error) => {
                let _ = self.events.send(ui_events::ExplorePosted {
                    request,
                    result: Err(error.to_string()),
                });
                return;
            }
        };
        if let Some((_, pinned)) = retry_agent {
            self.state.agent = Some(pinned);
        }
        // Each prompt has its own MCP access, so an earlier recipient cannot answer a Retry.
        self.state.renew_access();
        let pass = self.start_classification_if_enabled(pass);
        self.state.pass = Some(pass.clone());
        let _ = self.events.send(ui_events::ExplorePosted {
            request: request.clone(),
            result: Ok(Arc::new(pass.clone())),
        });
        let attempt = pass.turns[&request.request].attempt.clone();
        let (prepared, agent) = match self.prepare(&request) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.state.pending = Some((request.instance.clone(), request.request.clone()));
                self.prompt_finished(
                    ui_events::ExploreFinished {
                        instance: request.instance,
                        request: request.request.clone(),
                        result: Err(error.to_string()),
                    },
                    &attempt,
                );
                return;
            }
        };
        let observer = DurableDispatch {
            began: std::sync::atomic::AtomicBool::default(),
            store: self.store.clone(),
            unit: request.checkpoint.review_unit.clone(),
            instance: request.instance.clone(),
            id: review_explore::DispatchId::Interview {
                request: request.request.clone(),
                attempt: attempt.clone(),
            },
            events: self.events.clone(),
        };
        let (receipt, cancellation) =
            self.prompts
                .send_observed(agent, prepared.prompt(), Some(Arc::new(observer)));
        self.state.prompt = Some(cancellation);
        let inbox = self.inbox.clone();
        std::thread::spawn(move || {
            if let Err(error) = receipt.wait() {
                inbox.deliver(Input::PromptFinished {
                    event: Box::new(ui_events::ExploreFinished {
                        instance: request.instance,
                        request: request.request,
                        result: Err(error.to_string()),
                    }),
                    attempt,
                });
            }
        });
    }

    fn prepare(
        &mut self,
        request: &TurnRequest,
    ) -> eyre::Result<(review_explore_runner::PreparedTurn, PinnedAgent)> {
        let comparison = self
            .state
            .comparison
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Start Explore first"))?;
        eyre::ensure!(
            comparison.checkpoint == request.checkpoint,
            "Request does not belong to this comparison"
        );
        let comparison = comparison.clone();
        let agent = self.active_agent()?;
        let feedback = self
            .state
            .pass
            .as_ref()
            .filter(|pass| pass.exploration.instance == request.instance)
            .map(|pass| {
                pass.coverage.feedback(
                    &pass.exploration.comparison,
                    &pass.pending_questions(),
                    self.exclusion.is_enabled(),
                )
            });
        let prepared = review_explore_runner::PreparedTurn::prepare_with_feedback(
            request,
            &comparison,
            &self.state.access,
            feedback.as_ref(),
        );
        self.state.pending = Some((request.instance.clone(), request.request.clone()));
        self.state.agent = Some(agent.clone());
        Ok((prepared, agent))
    }

    pub(crate) fn prompt_finished(&mut self, event: ui_events::ExploreFinished, attempt: &str) {
        if self.state.pending.as_ref() != Some(&(event.instance.clone(), event.request.clone())) {
            return;
        }
        if let Some(pass) = &self.state.pass {
            let result = self.store.update_explore(
                &pass.exploration.comparison.checkpoint.review_unit,
                &event.instance,
                |pass| {
                    if pass
                        .turns
                        .get(&event.request)
                        .is_none_or(|turn| turn.attempt != attempt)
                    {
                        return Ok(false);
                    }
                    if let Err(error) = &event.result {
                        return Ok(pass.exploration.failed(&event.request, error));
                    }
                    Ok(false)
                },
            );
            match result {
                Ok((true, pass)) => self.state.pass = Some(pass),
                Ok((false, _)) => return,
                Err(error) => {
                    let _ = self
                        .events
                        .send(ui_events::ExploreStorageFailed(error.to_string()));
                    return;
                }
            }
        }
        self.state.prompt = None;
        let _ = self.events.send(event);
    }

    pub(crate) fn persist_request(
        &mut self,
        request: &TurnRequest,
        retry_agent: Option<&Agent>,
    ) -> eyre::Result<ExplorePass> {
        eyre::ensure!(
            self.state.storage_error.is_none(),
            "{}",
            self.state.storage_error.as_deref().unwrap_or_default()
        );
        eyre::ensure!(
            !self.state.historical,
            "This pass is history; open the latest pass or start a New pass"
        );
        if self.state.pass.is_none() {
            eyre::ensure!(retry_agent.is_none(), "No Explore pass to retry");
            let mut exploration = Exploration::new(
                self.state
                    .comparison
                    .clone()
                    .ok_or_else(|| eyre::eyre!("Start Explore first"))?,
            );
            exploration.instance.clone_from(&request.instance);
            let mut pass = ExplorePass::new(exploration);
            pass.post(request)?;
            pass.last_agent_session = self
                .state
                .agent
                .as_ref()
                .and_then(PinnedAgent::known_agent)
                .as_ref()
                .and_then(ConversationBinding::from_agent);
            self.state.loaded_unit = Some(request.checkpoint.review_unit.clone());
            return Ok(self.store.create_explore(pass)?);
        }
        Ok(self
            .store
            .update_explore(&request.checkpoint.review_unit, &request.instance, |pass| {
                let new = pass.post(request).map_err(|e| e.to_string())?;
                if let Some(agent) = retry_agent {
                    pass.last_agent_session = ConversationBinding::from_agent(agent);
                }
                if new
                    && let Some(view) = &self.state.last_view
                    && view.instance == request.instance
                {
                    pass.turns
                        .get_mut(&request.request)
                        .expect("posted turn")
                        .editor_sequence = Some(view.sequence);
                }
                Ok(new)
            })?
            .1)
    }
}
