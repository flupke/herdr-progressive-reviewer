//! Interview turns: persist the reviewer's request, then prompt the pinned agent.

use std::sync::Arc;

use herdr_client::protocol::Agent;
use review_explore::{Comparison, ConversationBinding, Exploration, ExploreRound, TurnRequest};
use review_explore_runner::{EarlierDecisions, Unreviewed};
use review_thread_service::PinnedAgent;

use vision_turns::TurnLog;

use crate::turn_log::SentTurn;
use crate::{ExploreSession, Input, dispatch::DurableDispatch};

/// A turn saved in the round, and the attempt that delivers it to the agent.
#[derive(Clone)]
pub(crate) struct SavedTurn {
    pub(crate) request: TurnRequest,
    pub(crate) attempt: String,
}

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

    /// Saves the turn `posted`, then prompts the agent with it, and tells the front ends
    /// through `ExplorePosted`. Errs, with the reason, only when the turn was not saved: a
    /// prompt that fails after the save leaves the turn waiting for Retry. A new answer that
    /// picks a choice run-ahead prepared is saved under the identities reserved for it; the
    /// front ends hear of the turn as they posted it.
    pub(crate) fn deliver_turn(
        &mut self,
        posted: TurnRequest,
        retry_agent: Option<(&Agent, PinnedAgent)>,
    ) -> Result<(), String> {
        // A kickoff saves the agent it selects with the new round.
        let kickoff = self.state.round.is_none();
        // Preserve the posted contribution even when its subsequent wakeup cannot be sent.
        if retry_agent.is_none() && kickoff {
            self.admit_kickoff(&posted)?;
        }
        // A Retry finds no question waiting: its turn keeps the identities it was saved with.
        let request = self.with_reserved_ids(posted.clone());
        let persisted =
            self.persist_request(&request, retry_agent.as_ref().map(|(agent, _)| *agent));
        let round = match persisted {
            Ok(round) => round,
            Err(error) => return Err(self.turn_refused(posted, &error)),
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
            request: posted,
            result: Ok(Arc::new(round.clone())),
        });
        let turn = SavedTurn {
            attempt: round.turns[&request.request].attempt.clone(),
            request,
        };
        // A turn a fork prepared needs no prompt: the pane's agent continues as the fork. A
        // turn saved while the agent switches to a fork waits for the switch.
        if !self.run_ahead_answer(&turn) && !self.run_ahead_hold(&turn) {
            self.send_turn(turn);
        }
        Ok(())
    }

    /// Prompts the agent with the saved turn `turn`; a prompt that fails leaves the turn
    /// waiting for Retry.
    pub(crate) fn send_turn(&mut self, turn: SavedTurn) {
        let SavedTurn { request, attempt } = turn;
        let (prompt, sent, agent) = match self.prepare(&request) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.state.pend(&request);
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
        let observer = self.dispatch_of(&request, &attempt, self.turns.clone().zip(sent));
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
    }

    /// What records the delivery of the turn `request`, as its attempt `attempt`, in the saved
    /// round; `turn` is the prompt, for a vision session's record of what was sent.
    pub(crate) fn dispatch_of(
        &self,
        request: &TurnRequest,
        attempt: &str,
        turn: Option<(TurnLog, SentTurn)>,
    ) -> DurableDispatch {
        DurableDispatch {
            began: std::sync::atomic::AtomicBool::default(),
            rounds: self.rounds.clone(),
            unit: request.checkpoint.review_unit.clone(),
            instance: request.instance.clone(),
            id: review_explore::DispatchId::Interview {
                request: request.request.clone(),
                attempt: attempt.to_owned(),
            },
            events: self.events.clone(),
            turn,
        }
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
        let comparison = self.turn_comparison(request)?;
        let agent = self.active_agent()?;
        let access = self.state.access.clone();
        let (prompt, unreviewed) = self.turn_prompt(request, &comparison, &access)?;
        let sent = self.turns.is_some().then(|| {
            SentTurn::interview(
                request,
                &self.state.access,
                unreviewed.to_string().trim().to_owned(),
                prompt.clone(),
            )
        });
        self.state.pend(request);
        self.state.agent = Some(agent.clone());
        Ok((prompt, sent, agent))
    }

    /// The change the round of `request` explores.
    pub(crate) fn turn_comparison(&self, request: &TurnRequest) -> eyre::Result<Arc<Comparison>> {
        let comparison = self
            .state
            .comparison
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Start Explore first"))?;
        eyre::ensure!(
            comparison.checkpoint == request.checkpoint,
            "Request does not belong to this comparison"
        );
        Ok(comparison.clone())
    }

    /// The agent's prompt for `request` of `comparison`, granting `access`, and the unreviewed
    /// lines it lists, whose diffs the session keeps until the next prompt.
    pub(crate) fn turn_prompt(
        &mut self,
        request: &TurnRequest,
        comparison: &Comparison,
        access: &str,
    ) -> eyre::Result<(String, Unreviewed)> {
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
            comparison,
            access,
            &unreviewed,
            &earlier,
            answered,
        )
        .prompt();
        Ok((prompt, unreviewed))
    }

    pub(crate) fn prompt_finished(&mut self, event: ui_events::ExploreFinished, attempt: &str) {
        if !self.state.is_pending(&event.instance, &event.request) {
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
