//! Interview turns: persist the reviewer's request, then prompt the pinned agent.

use std::sync::Arc;

use herdr_client::protocol::Agent;
use review_explore::{ConversationBinding, Exploration, ExploreRound, TurnRequest};
use review_explore_runner::EarlierDecisions;
use review_thread_service::PinnedAgent;

use crate::{ExploreSession, Input, dispatch::DurableDispatch, turn_log::SentTurn};

impl ExploreSession {
    /// Sends the turn `request` again to the selected agent, and tells the front ends through
    /// `ExplorePosted`. Errs, with the reason, only when the turn was not saved again.
    pub(crate) fn retry(&mut self, request: TurnRequest) -> Result<(), String> {
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
                return Err(error.to_string());
            }
        };
        self.deliver_turn(request, Some((&retry.0, retry.1)))
    }

    /// Saves the turn `request`, then prompts the agent with it, and tells the front ends
    /// through `ExplorePosted`. Errs, with the reason, only when the turn was not saved: a
    /// prompt that fails after the save leaves the turn waiting for Retry.
    pub(crate) fn deliver_turn(
        &mut self,
        request: TurnRequest,
        retry_agent: Option<(&Agent, PinnedAgent)>,
    ) -> Result<(), String> {
        // A kickoff saves the agent it selects with the new round.
        let kickoff = self.state.round.is_none();
        // Preserve the posted contribution even when its subsequent wakeup cannot be sent.
        if retry_agent.is_none() && kickoff {
            self.admit_kickoff(&request)?;
        }
        let persisted =
            self.persist_request(&request, retry_agent.as_ref().map(|(agent, _)| *agent));
        let round = match persisted {
            Ok(round) => round,
            Err(error) => return Err(self.turn_refused(request, &error)),
        };
        self.state.prompt = None;
        self.state.implementation = None;
        self.state.start.settle();
        if retry_agent.is_none() && !kickoff {
            let _ = self.select_agent();
        }
        // Before the prompt, so its unreviewed diffs leave out what the answer marked.
        let round = self.apply_answered_marks(&request, &round).unwrap_or(round);
        if let Some((_, pinned)) = retry_agent {
            self.state.agent = Some(pinned);
        }
        // Each prompt has its own MCP access, so an earlier recipient cannot answer a Retry.
        self.state.renew_access();
        self.state.round = Some(round.clone());
        let _ = self.events.send(ui_events::ExplorePosted {
            request: request.clone(),
            result: Ok(Arc::new(round.clone())),
        });
        let attempt = round.turns[&request.request].attempt.clone();
        let (prompt, sent, agent) = match self.prepare(&request) {
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
                return Ok(());
            }
        };
        let observer = DurableDispatch {
            began: std::sync::atomic::AtomicBool::default(),
            rounds: self.rounds.clone(),
            unit: request.checkpoint.review_unit.clone(),
            instance: request.instance.clone(),
            id: review_explore::DispatchId::Interview {
                request: request.request.clone(),
                attempt: attempt.clone(),
            },
            events: self.events.clone(),
            turn: self.turns.clone().zip(sent),
        };
        let (receipt, cancellation) =
            self.prompts
                .send_observed(agent, prompt, Some(Arc::new(observer)));
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
        Ok(())
    }

    /// Refuses the kickoff `request` when its start was stopped, or when nothing is left to
    /// review, as Jev or another reviewer may have marked the rest since the start; otherwise
    /// selects the agent the new round saves.
    fn admit_kickoff(&mut self, request: &TurnRequest) -> Result<(), String> {
        // A kickoff that arrives after its start was stopped, from the page while the pane
        // posted it, starts nothing, and leaves no failed start.
        if self.state.start.starting().is_none() {
            let error = "The start of this round was stopped".to_owned();
            let _ = self.events.send(ui_events::ExplorePosted {
                request: request.clone(),
                result: Err(error.clone()),
            });
            return Err(error);
        }
        if let Some(block) = self.kickoff_block() {
            return Err(self.turn_refused(request.clone(), &eyre::eyre!("{block}")));
        }
        let _ = self.select_agent();
        Ok(())
    }

    /// Tells the front ends that the turn `request` was not saved, for `error`, and returns
    /// the reason. A refused turn leaves the pending prompt, its implementation and its agent
    /// alone: a second answer must not cancel the first one's queued prompt. A refused kickoff
    /// ends the start.
    fn turn_refused(&mut self, request: TurnRequest, error: &eyre::Report) -> String {
        let error = error.to_string();
        if self.state.round.is_none() {
            self.state.start_failed(&error);
        }
        let _ = self.events.send(ui_events::ExplorePosted {
            request,
            result: Err(error.clone()),
        });
        error
    }

    /// The prompt for `request`, the record of it, and the agent to send it to.
    fn prepare(
        &mut self,
        request: &TurnRequest,
    ) -> eyre::Result<(String, Option<SentTurn>, PinnedAgent)> {
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
        let unreviewed = self.unreviewed().inspect_err(|error| {
            let _ = self.events.send(ui_events::ToastRequested {
                text: format!("{error:#}"),
                kind: toasts::ToastKind::Error,
            });
        })?;
        // Only a kickoff lists them; a later turn's agent already has them.
        let earlier = if request.is_kickoff() {
            let rounds = self
                .rounds
                .earlier(&request.checkpoint.review_unit, &request.instance)?;
            EarlierDecisions::new(rounds.standing.iter().map(|round| &round.exploration))
                .after_reset(rounds.reset_rounds)
        } else {
            EarlierDecisions::default()
        };
        let answered = self
            .state
            .round
            .as_ref()
            .zip(request.answer.as_ref())
            .and_then(|(round, answer)| round.exploration.answered_number(answer))
            .map(Into::into);
        let prompt = review_explore_runner::PreparedTurn::prepare(
            request,
            &comparison,
            &self.state.access,
            &unreviewed,
            &earlier,
            answered,
        )
        .prompt();
        let sent = self.turns.is_some().then(|| {
            SentTurn::interview(
                request,
                &self.state.access,
                unreviewed.to_string().trim().to_owned(),
                prompt.clone(),
            )
        });
        self.state.pending = Some((request.instance.clone(), request.request.clone()));
        self.state.agent = Some(agent.clone());
        Ok((prompt, sent, agent))
    }

    pub(crate) fn prompt_finished(&mut self, event: ui_events::ExploreFinished, attempt: &str) {
        if self.state.pending.as_ref() != Some(&(event.instance.clone(), event.request.clone())) {
            return;
        }
        if let Some(round) = &self.state.round {
            let result = self.rounds.update(
                &round.exploration.comparison.checkpoint.review_unit,
                &event.instance,
                |round| {
                    if round
                        .turns
                        .get(&event.request)
                        .is_none_or(|turn| turn.attempt != attempt)
                    {
                        return Ok(false);
                    }
                    if let Err(error) = &event.result {
                        return Ok(round.exploration.failed(&event.request, error));
                    }
                    Ok(false)
                },
            );
            match result {
                Ok((true, round)) => self.state.round = Some(round),
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
    ) -> eyre::Result<ExploreRound> {
        eyre::ensure!(
            self.state.storage_error.is_none(),
            "{}",
            self.state.storage_error.as_deref().unwrap_or_default()
        );
        eyre::ensure!(
            !self.state.historical,
            "This round is history; open the latest round or Reset to start a new one"
        );
        if self.state.round.is_none() {
            eyre::ensure!(retry_agent.is_none(), "No Explore round to retry");
            let mut exploration = Exploration::new(
                self.state
                    .comparison
                    .clone()
                    .ok_or_else(|| eyre::eyre!("Start Explore first"))?,
            );
            exploration.instance.clone_from(&request.instance);
            exploration.challenger = request.challenger;
            exploration.writing = request.writing;
            let mut round = ExploreRound::new(exploration);
            round.post(request)?;
            round.last_agent_session = self
                .state
                .agent
                .as_ref()
                .and_then(PinnedAgent::known_agent)
                .as_ref()
                .and_then(ConversationBinding::from_agent);
            self.state.loaded_unit = Some(request.checkpoint.review_unit.clone());
            return Ok(self.rounds.create(round)?);
        }
        Ok(self
            .rounds
            .update(
                &request.checkpoint.review_unit,
                &request.instance,
                |round| {
                    let new = round.post(request).map_err(|e| e.to_string())?;
                    if let Some(agent) = retry_agent {
                        round.last_agent_session = ConversationBinding::from_agent(agent);
                    }
                    if new
                        && let Some(view) = &self.state.last_view
                        && view.instance == request.instance
                    {
                        round
                            .turns
                            .get_mut(&request.request)
                            .expect("posted turn")
                            .editor_sequence = Some(view.sequence);
                    }
                    Ok(new)
                },
            )?
            .1)
    }
}
