//! The switch of the pane's agent to the fork the reviewer's answer chose. The answer's turn is
//! the agent's from the start of the switch, as if its prompt were on its way: the page shows
//! the agent working. Once Herdr reports the agent on the fork's session, the fork's turn
//! becomes the round's, under the identities the fork was told, which are the answer's, and
//! only that session may call the reviewer. A switch that fails leaves the turn interrupted
//! with the reason, and Retry sends the answer to the agent in the pane: the round is never
//! ahead of the agent. When the agent may run the fork's session but the round does not take
//! the fork's turn, the agent goes back to the session it ran before (`settle.rs`), so that it
//! never takes the answer twice. No prompt reaches the agent while it switches.

use herdr_client::protocol::Agent;
use review_run_ahead::{
    Continuation, DiscardReason, ForkPoint, PlainReason, SwitchFailure, TurnPath,
};
use review_thread_service::{DispatchObserver, PromptError, PromptHold};

use super::settle::Trigger;
use super::take::ForkFiles;
use super::{Asked, Event, Move, RoundKey, RunAheadInput, TakenFork};
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
    /// No prompt reaches the agent while it switches, the thread service's included.
    pub(super) hold: PromptHold,
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

impl ExploreSession {
    /// Starts `switching`: the answer's turn is the pane agent's from now on, on its way to
    /// it. Returns why the answer goes to the agent instead, after discarding the fork, when
    /// the turn could not be recorded as on its way.
    pub(super) fn start_switch(&mut self, switching: Switching) -> Result<(), PlainReason> {
        // Recorded before the agent is told anything, so that a reviewer that stops meanwhile
        // leaves the switch for the next one to settle.
        let began = self
            .record_continuation(
                &switching,
                Continuation::Switching {
                    at_ms: review_explore::now_ms(),
                },
            )
            .and_then(|()| {
                self.agents
                    .get_agent(&switching.asked.pane)
                    .map_err(|error| error.to_string())
            })
            .and_then(|agent| agent.ok_or_else(|| "the agent's pane is gone".to_owned()))
            .and_then(|agent| switching.dispatch.before_attempt(&agent));
        if let Err(error) = began {
            // Nothing was typed: the fork's session is not the agent's.
            let _ = self.record_continuation(
                &switching,
                Continuation::Failed {
                    at_ms: review_explore::now_ms(),
                    error: error.clone(),
                    typed: false,
                },
            );
            self.discard_switching_fork(&switching);
            return Err(PlainReason::SwitchFailed { error });
        }
        let round = &switching.asked.round;
        if let Ok(Some(saved)) = self.rounds.round(&round.unit, &round.instance) {
            self.state.round = Some(saved);
        }
        self.state.prompt = None;
        self.state.pend(&switching.turn.request);
        let inbox = self.inbox.clone();
        let (round, request) = (
            switching.asked.round.clone(),
            switching.turn.request.request.clone(),
        );
        Move::switch(switching.fork.trace(&switching.point)).start_once_drained(
            &self.run_ahead.host,
            &switching.hold,
            &switching.asked.pane,
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
        // A question that waits again, after a Cancel answer, is forked once the agent runs the
        // session the round needs.
        self.after_agent_moved();
    }

    /// Holds the saved turn `turn` while the pane's agent switches to a fork's session: no
    /// prompt reaches the agent before it runs that session. Returns whether it holds it; it is
    /// sent once the switch ends.
    pub(crate) fn run_ahead_hold(&mut self, turn: &SavedTurn) -> bool {
        let Some(switching) = &self.run_ahead.switching else {
            return self.settle_before(turn);
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

    /// The pane's agent `agent` runs the fork's session of `switching`: the fork's turn becomes
    /// the round's, and that session alone may call the reviewer from now on, unless the
    /// reviewer moved the round on meanwhile or the turn could not be saved; the agent then goes
    /// back to the session it ran before.
    fn continue_as_fork(&mut self, switching: Switching, agent: &Agent) {
        self.log_switch(
            &switching,
            &format!(
                "the agent in the pane runs the fork's session {}",
                switching.fork.session
            ),
        );
        let request = &switching.turn.request;
        let delivering = self.state.is_pending(&request.instance, &request.request)
            && self
                .state
                .round
                .as_ref()
                .is_some_and(|round| round.exploration.instance == request.instance);
        let Some(turn) = switching.fork.kept.clone().filter(|_| delivering) else {
            self.log_switch(
                &switching,
                "the answer's turn no longer waits: the fork's turn is not saved",
            );
            self.record_plain(&switching, PlainReason::Withdrawn);
            switching.finish(&Err(PromptError::Cancelled));
            self.undo_switch(
                &switching,
                "the reviewer withdrew the answer while the agent switched",
            );
            return;
        };
        self.follow_agent(&switching.asked.round, agent);
        match self.commit(&turn) {
            Ok((_, round)) => {
                // Recorded once the round holds the fork's turn: a reviewer that stops before
                // leaves the switch unsettled, for the next one to settle.
                let _ = self.record_continuation(
                    &switching,
                    Continuation::Switched {
                        at_ms: review_explore::now_ms(),
                    },
                );
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
                self.undo_switch(&switching, &error);
            }
        }
    }

    /// The switch of `switching` failed: the turn waits for Retry, which sends the answer to
    /// the agent in the pane. When the agent was told to resume the fork's session, which it
    /// may still do, it goes back to the session it ran before.
    fn switch_failed(&mut self, switching: &Switching, failure: &SwitchFailure) {
        self.log_switch(
            switching,
            &format!(
                "the agent in the pane did not switch to the fork's session: {}",
                failure.error
            ),
        );
        let _ = self.record_continuation(
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
        if failure.typed {
            self.undo_switch(switching, &failure.error);
        }
    }

    /// The agent may run the fork's session of `switching`, whose turn the round does not
    /// take, for `reason`: it goes back to the session it ran before.
    fn undo_switch(&mut self, switching: &Switching, reason: &str) {
        let round = &switching.asked.round;
        let record = self
            .rounds
            .forks(&round.unit, &round.instance)
            .ok()
            .and_then(|forks| forks.fork(&switching.fork.session).cloned());
        let Some(record) = record else {
            self.log_switch(
                switching,
                "the fork's record is gone: the agent stays on the session it runs",
            );
            return;
        };
        let pane = switching.asked.pane.clone();
        self.settle(round, record, &pane, reason.to_owned(), Trigger::RunAhead);
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

    /// Records where the switch of `switching` stands, on its fork's record; logs it when it was
    /// not recorded, and errs with the reason, which only the start of a switch needs.
    fn record_continuation(
        &self,
        switching: &Switching,
        continued: Continuation,
    ) -> Result<(), String> {
        let saved = self.update_forks(&switching.asked.round, |forks| {
            forks
                .fork_mut(&switching.fork.session)
                .map(|record| record.continued = Some(continued))
        });
        let error = match saved {
            Ok(Some(())) => return Ok(()),
            Ok(None) => "the switch was not recorded: the fork's record is gone".to_owned(),
            Err(error) => format!("the switch was not recorded: {error}"),
        };
        self.log_switch(switching, &error);
        Err(error)
    }

    /// Adds `line` about `switching` to run-ahead's log.
    fn log_switch(&self, switching: &Switching, line: &str) {
        self.run_ahead
            .host
            .log(&format!("{}: {line}", switching.asked.label()));
    }
}
