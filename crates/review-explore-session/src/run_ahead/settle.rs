//! Settling the session the pane's agent runs, after a switch to a fork that did not end with
//! the round taking the fork's turn: the switch failed once the agent was told to resume the
//! fork's session, which it may still do late; the reviewer withdrew the answer, or the fork's
//! turn could not be saved, once the agent ran the fork's session; or the reviewer stopped
//! while the switch ran.
//!
//! The agent's session must match the round: a session that holds a turn the round does not
//! have would take the answer a second time. So before any prompt reaches the agent, and
//! before forks are taken again, the agent resumes the session the forks were taken from, which
//! holds neither the answer nor the fork's turn, and its next prompt runs the plain chain. Only
//! when the round took the fork's turn does the agent resume the fork's session instead. No
//! prompt reaches the agent while it settles; a turn saved meanwhile waits, then goes out, or
//! waits for Retry when the agent could not be settled.

use herdr_client::protocol::{Agent, PaneId};
use review_explore::ConversationBinding;
use review_run_ahead::{Continuation, DiscardReason, ForkRecord, SwitchFailure, TurnPath};
use review_thread_service::{PinnedAgent, PromptHold};

use super::{Event, Move, RoundKey, RunAheadInput};
use crate::{ExploreSession, Input};

/// The pane's agent being put on the session its round needs.
pub(super) struct Settling {
    round: RoundKey,
    /// The fork whose session the agent may run.
    fork: ForkRecord,
    target: Target,
    /// Why the switch did not end with the round taking the fork's turn.
    reason: String,
    trigger: Trigger,
    /// No prompt reaches the agent while it settles.
    _hold: PromptHold,
}

/// What has the agent settle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Trigger {
    /// A prompt to the agent, which waits for it.
    Prompt,
    /// Run-ahead itself: after a switch, or before it takes forks.
    RunAhead,
}

/// The session the agent is put on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Target {
    /// The session the fork was taken from: the round did not take the fork's turn.
    Origin,
    /// The fork's session: the round took the fork's turn.
    Fork,
}

impl ExploreSession {
    /// Puts the agent of `pane`, which may run the session of `fork` of `round`, on the
    /// session the round needs, for `reason`, as `trigger` has it. Returns whether it started.
    pub(super) fn settle(
        &mut self,
        round: &RoundKey,
        fork: ForkRecord,
        pane: &PaneId,
        reason: String,
        trigger: Trigger,
    ) -> bool {
        if self.run_ahead.settling.is_some() {
            return true;
        }
        let target = if self.round_took(round, &fork) {
            Target::Fork
        } else {
            Target::Origin
        };
        let inbox = self.inbox.clone();
        let (key, session) = (round.clone(), fork.session.clone());
        let done = Box::new(move |result| {
            inbox.deliver(Input::RunAhead(RunAheadInput(Event::Settled {
                round: key,
                fork: session,
                result: Box::new(result),
            })));
        });
        let to = match target {
            Target::Origin => {
                let Some(from) = &fork.from else {
                    self.run_ahead.host.log(&format!(
                        "the agent may run the session of the fork {}, whose origin is not \
                         recorded: it stays where it is",
                        fork.session
                    ));
                    return false;
                };
                self.run_ahead.host.log(&format!(
                    "{reason}: the agent goes back to the session {from} it ran before the fork {}",
                    fork.session
                ));
                Move::Resume(from.clone())
            }
            Target::Fork => {
                self.run_ahead.host.log(&format!(
                    "{reason}: the round took the turn of the fork {}, whose session the agent \
                     resumes",
                    fork.session
                ));
                Move::switch(fork.trace())
            }
        };
        // Held before anything is typed in the pane.
        let hold = self.prompts.hold();
        to.start_once_drained(&self.run_ahead.host, &hold, pane, done);
        self.run_ahead.settling = Some(Settling {
            round: round.clone(),
            fork,
            target,
            reason,
            trigger,
            _hold: hold,
        });
        true
    }

    /// Whether the round `round` took the turn of the fork `fork`.
    fn round_took(&self, round: &RoundKey, fork: &ForkRecord) -> bool {
        let Ok(forks) = self.rounds.forks(&round.unit, &round.instance) else {
            return false;
        };
        let Some(request) = forks.continued_as(&fork.session) else {
            return false;
        };
        self.rounds
            .round(&round.unit, &round.instance)
            .ok()
            .flatten()
            .is_some_and(|saved| {
                saved
                    .exploration
                    .conversation
                    .iter()
                    .any(|turn| turn.update.request == request)
            })
    }

    /// The fork of the session's round whose session the pane's agent may run, though no
    /// switch to it runs in this reviewer, nor in another that still runs.
    pub(super) fn unsettled(&self) -> Option<(RoundKey, ForkRecord)> {
        let saved = self.state.round.as_ref()?;
        let round = RoundKey {
            unit: saved.exploration.comparison.checkpoint.review_unit.clone(),
            instance: saved.exploration.instance.clone(),
        };
        let switching = self
            .run_ahead
            .switching
            .as_ref()
            .map(|switching| switching.fork.session.as_str());
        let reviewer = self.run_ahead.reviewer;
        let fork = self
            .rounds
            .forks(&round.unit, &round.instance)
            .ok()?
            .forks
            .into_iter()
            .find(|fork| {
                fork.is_unsettled()
                    && switching != Some(fork.session.as_str())
                    && !self.run_ahead.left_unrecorded.contains(&fork.session)
                    && fork.may_be_settled_by(reviewer)
            })?;
        Some((round, fork))
    }

    /// Holds the saved turn `turn` while the pane's agent may run a fork's session the round
    /// did not take, and starts to settle it. Returns whether it holds the turn.
    pub(super) fn settle_before(&mut self, turn: &crate::turn::SavedTurn) -> bool {
        if self.run_ahead.settling.is_none() {
            let Some((round, fork)) = self.unsettled() else {
                return false;
            };
            let Some(pane) = self.round_agent().map(|agent| agent.pane_id) else {
                return false;
            };
            let reason = format!(
                "a prompt waits while the agent may run the session of the fork {}",
                fork.session
            );
            if !self.settle(&round, fork, &pane, reason, Trigger::Prompt) {
                return false;
            }
        }
        self.run_ahead
            .host
            .log("a turn waits for the agent to settle before its prompt goes out");
        self.state.pend(&turn.request);
        self.state.prompt = None;
        self.run_ahead.held = Some(turn.clone());
        true
    }

    /// The settling of the agent, which may have run the session of the fork `fork` of
    /// `round`, ended with `result`.
    pub(super) fn run_ahead_settled(
        &mut self,
        round: &RoundKey,
        fork: &str,
        result: Result<Agent, SwitchFailure>,
    ) {
        let Some(settling) = self
            .run_ahead
            .settling
            .take_if(|settling| settling.round == *round && settling.fork.session == fork)
        else {
            return;
        };
        match result {
            Ok(agent) => self.settled(&settling, &agent),
            Err(failure) => self.not_settled(&settling, &failure),
        }
    }

    /// The agent `agent` runs the session the round needs: the prompt that waited goes out,
    /// and forks may be taken again.
    fn settled(&mut self, settling: &Settling, agent: &Agent) {
        let Settling {
            round,
            fork,
            target,
            reason,
            ..
        } = settling;
        let now = review_explore::now_ms();
        let saved = self.update_forks(round, |forks| {
            let Some(record) = forks.fork_mut(&fork.session) else {
                return;
            };
            record.continued = Some(match target {
                Target::Origin => Continuation::Undone {
                    at_ms: now,
                    reason: reason.clone(),
                },
                Target::Fork => Continuation::Switched { at_ms: now },
            });
            // The agent left the fork's session: it goes, as a fork the answer did not use.
            if *target == Target::Origin {
                record.discard(DiscardReason::Answered, now);
            }
        });
        if let Err(error) = saved {
            self.run_ahead
                .host
                .log(&format!("the agent's session was not recorded: {error}"));
            // The record still says the agent may run the fork's session: this reviewer knows
            // better.
            self.run_ahead.left_unrecorded.insert(fork.session.clone());
        }
        match target {
            Target::Origin => {
                self.discard_in_background(round, fork.trace());
                self.run_ahead.host.log(&format!(
                    "the agent runs its own session again; the fork {} is discarded",
                    fork.session
                ));
            }
            Target::Fork => {
                if let Ok(forks) = self.rounds.forks(&round.unit, &round.instance)
                    && let Some(request) = forks.continued_as(&fork.session)
                {
                    let path = TurnPath::Prepared {
                        session: fork.session.clone(),
                    };
                    self.show_path(round, request, path);
                }
                self.run_ahead.host.log(&format!(
                    "the agent runs the session of the fork {}",
                    fork.session
                ));
            }
        }
        self.run_ahead.settle_failed.remove(&fork.session);
        self.follow_agent(round, agent);
        self.after_agent_moved();
    }

    /// The agent was not put on the session the round needs, for `failure`: its session stays
    /// unsettled, and a prompt that waited waits for Retry.
    fn not_settled(&mut self, settling: &Settling, failure: &SwitchFailure) {
        let Settling {
            round,
            fork,
            reason,
            trigger,
            ..
        } = settling;
        self.run_ahead.host.log(&format!(
            "the agent was not settled after the fork {}: {}",
            fork.session, failure.error
        ));
        // A resume typed in vain is not typed again unasked, lest it be typed again and again.
        if *trigger == Trigger::RunAhead && failure.typed {
            self.run_ahead.settle_failed.insert(fork.session.clone());
        }
        let now = review_explore::now_ms();
        let _ = self.update_forks(round, |forks| {
            if let Some(record) = forks.fork_mut(&fork.session)
                && matches!(record.continued, Some(Continuation::Switching { .. }))
            {
                record.continued = Some(Continuation::Failed {
                    at_ms: now,
                    error: format!("{reason}; {}", failure.error),
                    typed: true,
                });
            }
        });
        if let Some(held) = self.run_ahead.held.take()
            && self.still_waits(&held)
        {
            self.prompt_finished(
                ui_events::ExploreFinished {
                    instance: held.request.instance.clone(),
                    request: held.request.request.clone(),
                    result: Err(format!(
                        "The agent in the pane may still run the session of a turn prepared \
                         while you were thinking, and it could not be put back on its own \
                         session: {}. Look at the agent's pane, then Retry.",
                        failure.error
                    )),
                },
                &held.attempt,
            );
        }
    }

    /// The pane's agent `agent` ran a new session for `round`: the session pins it, and the
    /// round records it as the agent's latest session.
    /// Nothing changes for a round the session no longer runs, as after a Reset.
    pub(super) fn follow_agent(&mut self, round: &RoundKey, agent: &Agent) {
        if self
            .state
            .round
            .as_ref()
            .is_none_or(|saved| saved.exploration.instance != round.instance)
        {
            return;
        }
        self.state.agent = Some(PinnedAgent::new(agent.clone()));
        let followed = self.rounds.update(&round.unit, &round.instance, |saved| {
            saved.last_agent_session = ConversationBinding::from_agent(agent);
            Ok(())
        });
        match followed {
            Ok(((), saved)) => self.state.round = Some(saved),
            Err(error) => self
                .run_ahead
                .host
                .log(&format!("the agent's session was not recorded: {error}")),
        }
    }

    /// The agent's session moved, by a switch or a settling: unless it must settle still, the
    /// prompt that waited goes out, and a question that waits is forked.
    pub(super) fn after_agent_moved(&mut self) {
        if self.run_ahead.switching.is_some() || self.run_ahead.settling.is_some() {
            return;
        }
        // Another fork's session may still need settling before the prompt goes out.
        if let Some(held) = self.run_ahead.held.take()
            && self.still_waits(&held)
            && !self.settle_before(&held)
        {
            self.send_turn(held);
        }
        if self
            .run_ahead
            .armed
            .as_ref()
            .is_some_and(|armed| armed.taken.is_none())
        {
            self.run_ahead_take();
        }
    }

    /// Whether the held turn `held` still waits for its prompt.
    fn still_waits(&self, held: &crate::turn::SavedTurn) -> bool {
        self.state
            .is_pending(&held.request.instance, &held.request.request)
    }
}
