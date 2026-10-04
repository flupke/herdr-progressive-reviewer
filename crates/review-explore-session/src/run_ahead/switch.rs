//! The switch of the pane's agent to the fork the reviewer's answer chose. The answer's turn is
//! the agent's from the start of the switch, as if its prompt were on its way: the page shows
//! the agent working. Once Herdr reports the agent on the fork's session, the fork's turn
//! becomes the round's, under the identities of the reviewer's answer, and only that session
//! may call the reviewer. A switch that fails leaves the turn interrupted with the reason, and
//! Retry sends the answer to the agent in the pane: the round is never ahead of the agent.

use herdr_client::protocol::Agent;
use review_explore::{ConversationBinding, InterviewUpdate, TurnRequest};
use review_run_ahead::{
    Continuation, DiscardReason, ForkPoint, PlainReason, SwitchFailure, SwitchTo, TurnPath,
};
use review_thread_service::{DispatchObserver, PinnedAgent, PromptError};

use super::take::ForkFiles;
use super::{Asked, Event, RoundKey, RunAheadInput, TakenFork};
use crate::dispatch::DurableDispatch;
use crate::turn::SavedTurn;
use crate::{ExploreSession, Input};

/// A switch of the pane's agent to a fork's session, from before it starts until it ends.
pub(super) struct Switching {
    /// The question the reviewer answered.
    pub(super) asked: Asked,
    /// The reviewer's answer's turn, as saved, and what records its delivery.
    pub(super) turn: SavedTurn,
    pub(super) dispatch: DurableDispatch,
    /// The fork, with where it was taken from and the files its prompt names.
    pub(super) point: ForkPoint,
    pub(super) fork: TakenFork,
    pub(super) files: ForkFiles,
}

impl Switching {
    fn is_for(&self, round: &RoundKey, request: &str) -> bool {
        self.asked.round == *round && self.turn.request.request == request
    }

    /// Records in the saved round how the delivery of the answer's turn ended, as a prompt's
    /// delivery would end with `result`. The dispatch reports a storage failure itself.
    fn finish(&self, result: &Result<(), PromptError>) {
        let _ = self.dispatch.finished(result);
    }
}

impl TakenFork {
    /// The turn the fork submitted, as the turn of the reviewer's answer `real`: the same turn,
    /// under the identities of the answer.
    fn turn_for(&self, real: &TurnRequest) -> Option<InterviewUpdate> {
        let mut turn = self.kept.clone()?;
        turn.request.clone_from(&real.request);
        if let (Some(interpretation), Some(forked), Some(real)) =
            (&mut turn.interpretation, &self.request.answer, &real.answer)
            && interpretation.answer == forked.id
        {
            interpretation.answer.clone_from(&real.id);
        }
        Some(turn)
    }
}

impl ExploreSession {
    /// Starts `switching`: the answer's turn is the pane agent's from now on, on its way to
    /// it. Returns why the answer goes to the agent instead, after discarding the fork, when
    /// the turn could not be recorded as on its way.
    pub(super) fn start_switch(&mut self, switching: Switching) -> Result<(), PlainReason> {
        let began = self
            .agents
            .get_agent(&switching.asked.pane)
            .map_err(|error| error.to_string())
            .and_then(|agent| agent.ok_or_else(|| "the agent's pane is gone".to_owned()))
            .and_then(|agent| switching.dispatch.before_attempt(&agent));
        if let Err(error) = began {
            self.discard_switching_fork(&switching);
            return Err(PlainReason::SwitchFailed { error });
        }
        let round = &switching.asked.round;
        if let Ok(Some(saved)) = self.rounds.round(&round.unit, &round.instance) {
            self.state.round = Some(saved);
        }
        self.state.prompt = None;
        self.state.pend(&switching.turn.request);
        self.record_continuation(
            &switching,
            Continuation::Switching {
                at_ms: review_explore::now_ms(),
            },
        );
        let inbox = self.inbox.clone();
        let (round, request) = (
            switching.asked.round.clone(),
            switching.turn.request.request.clone(),
        );
        self.run_ahead.host.switch(
            SwitchTo {
                pane: &switching.asked.pane,
                fork: switching.fork.trace(&switching.point),
            },
            Box::new(move |result| {
                inbox.deliver(Input::RunAhead(RunAheadInput(Event::Switched {
                    round,
                    request,
                    result: Box::new(result),
                })));
            }),
        );
        self.run_ahead.switching = Some(switching);
        Ok(())
    }

    /// The switch for the turn `request` of `round` ended with `result`.
    pub(super) fn run_ahead_switched(
        &mut self,
        round: &RoundKey,
        request: &str,
        result: Result<Agent, SwitchFailure>,
    ) {
        let Some(switching) = self
            .run_ahead
            .switching
            .take_if(|switching| switching.is_for(round, request))
        else {
            return;
        };
        match result {
            Ok(agent) => self.continue_as_fork(switching, &agent),
            Err(failure) => self.switch_failed(&switching, &failure),
        }
        if let Some(held) = self.run_ahead.held.take() {
            self.send_held(held);
        }
        // A question that waits again, after a Cancel answer, is forked now that the agent runs
        // its session.
        if self
            .run_ahead
            .armed
            .as_ref()
            .is_some_and(|armed| armed.taken.is_none())
        {
            self.run_ahead_take();
        }
    }

    /// Holds the saved turn `turn` while the pane's agent switches to a fork's session: no
    /// prompt reaches the agent before it runs that session. Returns whether it holds it; it is
    /// sent once the switch ends.
    pub(crate) fn run_ahead_hold(&mut self, turn: &SavedTurn) -> bool {
        let Some(switching) = &self.run_ahead.switching else {
            return false;
        };
        self.log_switch(
            switching,
            "a turn waits for the switch to end before its prompt goes out",
        );
        self.state.pend(&turn.request);
        self.state.prompt = None;
        self.run_ahead.held = Some(turn.clone());
        true
    }

    /// Sends the turn `held` that waited, unless it no longer waits.
    fn send_held(&mut self, held: SavedTurn) {
        if self
            .state
            .is_pending(&held.request.instance, &held.request.request)
        {
            self.send_turn(held);
        }
    }

    /// The pane's agent `agent` runs the fork's session of `switching`: it alone may call the
    /// reviewer from now on, and the fork's turn becomes the round's, unless the reviewer moved
    /// the round on meanwhile.
    fn continue_as_fork(&mut self, switching: Switching, agent: &Agent) {
        self.record_continuation(
            &switching,
            Continuation::Switched {
                at_ms: review_explore::now_ms(),
            },
        );
        self.log_switch(
            &switching,
            &format!(
                "the agent in the pane runs the fork's session {}",
                switching.fork.session
            ),
        );
        self.state.agent = Some(PinnedAgent::new(agent.clone()));
        // Whatever became of the turn, the agent runs the fork's session now.
        let round = &switching.asked.round;
        let followed = self.rounds.update(&round.unit, &round.instance, |round| {
            round.last_agent_session = ConversationBinding::from_agent(agent);
            Ok(())
        });
        if let Err(error) = followed {
            self.log_switch(
                &switching,
                &format!("the agent's session was not recorded: {error}"),
            );
        }
        let request = &switching.turn.request;
        let delivering = self.state.is_pending(&request.instance, &request.request)
            && self
                .state
                .round
                .as_ref()
                .is_some_and(|round| round.exploration.instance == request.instance);
        let Some(turn) = switching.fork.turn_for(request).filter(|_| delivering) else {
            self.log_switch(
                &switching,
                "the answer's turn no longer waits: the fork's turn is not saved",
            );
            self.record_plain(&switching, PlainReason::Withdrawn);
            switching.finish(&Err(PromptError::Cancelled));
            return;
        };
        match self.commit(&turn) {
            Ok((_, round)) => {
                switching.finish(&Ok(()));
                self.log_switch(
                    &switching,
                    "the fork's turn is saved as the round's next turn",
                );
                let Switching {
                    asked,
                    turn: SavedTurn { request, .. },
                    fork,
                    files,
                    ..
                } = switching;
                // The agent's latest prompt is the fork's: its access, and the diffs it names.
                self.state.access = fork.access;
                self.diffs = Some(files.into_diffs());
                self.show_path(
                    &asked.round,
                    &request.request,
                    TurnPath::Prepared {
                        session: fork.session,
                    },
                );
                crate::publish_committed(&self.events, round);
            }
            Err(error) => {
                let error = format!("the fork's turn was not saved: {error}");
                switching.finish(&Err(PromptError::Delivery(error.clone())));
                self.log_switch(&switching, &error);
                self.record_plain(
                    &switching,
                    PlainReason::SwitchFailed {
                        error: error.clone(),
                    },
                );
                self.turn_failed(&switching, &error);
            }
        }
    }

    /// The switch of `switching` failed: the turn waits for Retry, which sends the answer to
    /// the agent in the pane.
    fn switch_failed(&mut self, switching: &Switching, failure: &SwitchFailure) {
        self.log_switch(
            switching,
            &format!(
                "the agent in the pane did not switch to the fork's session: {}",
                failure.error
            ),
        );
        self.record_continuation(
            switching,
            Continuation::Failed {
                at_ms: review_explore::now_ms(),
                error: failure.error.clone(),
                typed: failure.typed,
            },
        );
        self.record_plain(
            switching,
            PlainReason::SwitchFailed {
                error: failure.error.clone(),
            },
        );
        // The agent may run the fork's session once told to resume it: its transcript stays.
        if !failure.typed {
            self.discard_switching_fork(switching);
        }
        switching.finish(&Err(PromptError::Delivery(failure.error.clone())));
        self.turn_failed(
            switching,
            &format!(
                "The agent in the pane could not continue with the turn prepared while you were \
                 thinking: {}",
                failure.error
            ),
        );
    }

    /// The answer's turn of `switching` failed, for `error`: it waits for Retry.
    fn turn_failed(&mut self, switching: &Switching, error: &str) {
        self.prompt_finished(
            ui_events::ExploreFinished {
                instance: switching.turn.request.instance.clone(),
                request: switching.turn.request.request.clone(),
                result: Err(error.to_owned()),
            },
            &switching.turn.attempt,
        );
    }

    /// Discards the fork of `switching`: it is stopped and its transcript deleted.
    pub(super) fn discard_switching_fork(&mut self, switching: &Switching) {
        self.discard_forks(
            &switching.asked.round,
            &switching.point,
            std::slice::from_ref(&switching.fork),
            DiscardReason::Answered,
        );
    }

    /// Records that the answer of `switching` runs the plain chain after all, for `reason`.
    fn record_plain(&mut self, switching: &Switching, reason: PlainReason) {
        let path = TurnPath::Plain { reason };
        let request = &switching.turn.request.request;
        self.show_path(&switching.asked.round, request, path.clone());
        let saved = self.update_forks(&switching.asked.round, |forks| {
            if let Some(answer) = forks.answer_mut(request) {
                answer.path = path;
            }
        });
        if let Err(error) = saved {
            self.log_switch(
                switching,
                &format!("the answer's path was not recorded: {error}"),
            );
        }
    }

    /// Records where the switch of `switching` stands, on its fork's record.
    fn record_continuation(&self, switching: &Switching, continued: Continuation) {
        let saved = self.update_forks(&switching.asked.round, |forks| {
            if let Some(record) = forks.fork_mut(&switching.fork.session) {
                record.continued = Some(continued);
            }
        });
        if let Err(error) = saved {
            self.log_switch(switching, &format!("the switch was not recorded: {error}"));
        }
    }

    /// Adds `line` about `switching` to run-ahead's log.
    fn log_switch(&self, switching: &Switching, line: &str) {
        self.run_ahead
            .host
            .log(&format!("{}: {line}", switching.asked.label()));
    }
}
