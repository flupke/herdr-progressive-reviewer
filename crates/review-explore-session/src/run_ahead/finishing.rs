//! A bare answer whose fork has not submitted its turn yet: the fork's turn is the answer's,
//! and the session waits for it, while the pane and the page say that the agent finishes a turn
//! it started while the reviewer was thinking. Once the fork submits, the answer is checked
//! again, as at the answer, and the pane's agent switches to the fork at once (`switch.rs`),
//! before the fork's process ends: the next question shows only once the agent runs the fork's
//! session, so the reviewer's next answer reaches that session. Stop waiting, a fork that ends
//! without its turn, or a check that fails sends the answer to the agent in the pane, which
//! takes the turn from the start. No prompt is held meanwhile: one that reaches the agent moves
//! its session, and the check at the fork's submit then sends the answer to the agent.

use review_explore::TurnRequest;
use review_run_ahead::{DiscardReason, ForkPoint, PlainReason, TurnPath};

use super::switch::Switching;
use super::take::ForkFiles;
use super::{Asked, TakenFork};
use crate::ExploreSession;
use crate::turn::SavedTurn;

/// A bare answer that waits for the fork of its choice to submit its turn.
pub(super) struct Finishing {
    /// The question the reviewer answered, and when the agent asked it.
    pub(super) asked: Asked,
    pub(super) asked_at_ms: u64,
    /// The reviewer's answer's turn, as saved.
    pub(super) turn: SavedTurn,
    /// Where the forks were taken from, and when: the fork started its turn then.
    pub(super) point: ForkPoint,
    pub(super) taken_at_ms: u64,
    /// The fork, and the files its prompt names.
    pub(super) fork: TakenFork,
    pub(super) files: ForkFiles,
}

impl ExploreSession {
    /// The answer of `finishing` waits for its fork: its turn is the agent's from now on, and the
    /// front ends hear since when the fork works on it. Returns the path the answer's turn takes.
    pub(super) fn wait_for_fork(&mut self, finishing: Finishing) -> TurnPath {
        let request = &finishing.turn.request;
        self.state.prompt = None;
        self.state.pend(request);
        self.run_ahead.host.log(&format!(
            "{}: the fork for {} has not submitted its turn yet: the answer waits for it",
            finishing.asked.label(),
            finishing.fork.choice
        ));
        let _ = self.events.send(ui_events::ExploreTurnFinishing {
            round: request.instance.clone(),
            request: request.request.clone(),
            started_at_ms: finishing.taken_at_ms,
        });
        let session = finishing.fork.session.clone();
        self.run_ahead.finishing = Some(finishing);
        TurnPath::Prepared { session }
    }

    /// When the fork that the turn `request` waits for, or switches to after waiting, started
    /// its turn, in milliseconds since the epoch.
    pub(crate) fn run_ahead_finishing_since(&self, request: &str) -> Option<u64> {
        if let Some(finishing) = &self.run_ahead.finishing
            && finishing.turn.request.request == request
        {
            return Some(finishing.taken_at_ms);
        }
        self.run_ahead
            .switching
            .as_ref()
            .filter(|switching| switching.turn.request.request == request)
            .and_then(Switching::finishing_since)
    }

    /// The fork the answer waits for may have submitted its turn: once it did, the answer is
    /// checked again, and the pane's agent continues as the fork, or takes the turn itself.
    pub(super) fn run_ahead_fork_submitted(&mut self) {
        let Some(finishing) = self
            .run_ahead
            .finishing
            .take_if(|finishing| finishing.fork.kept.is_some())
        else {
            return;
        };
        let Finishing {
            asked,
            asked_at_ms,
            turn,
            point,
            taken_at_ms,
            fork,
            files,
        } = finishing;
        self.run_ahead.host.log(&format!(
            "{}: the fork the answer waited for submitted its turn",
            asked.label()
        ));
        let path = match self.unchanged_since_forks(&asked, asked_at_ms, &point, taken_at_ms) {
            Ok(()) => self.continue_as(Switching {
                asked: asked.clone(),
                dispatch: self.dispatch_of(&turn.request, &turn.attempt, None),
                turn: turn.clone(),
                point,
                fork,
                files,
                hold: self.prompts.hold(),
                finishing_since_ms: Some(taken_at_ms),
                stopped: false,
            }),
            Err(reason) => {
                self.discard_forks(
                    &asked.round,
                    &point,
                    std::slice::from_ref(&fork),
                    DiscardReason::Answered,
                );
                TurnPath::Plain { reason }
            }
        };
        if let TurnPath::Plain { reason } = path {
            self.answer_to_agent(&asked, turn, reason);
        }
    }

    /// The fork `session` ended: when the answer waits for it, it ended without its turn, and
    /// the answer goes to the agent in the pane.
    pub(super) fn run_ahead_waited_fork_ended(&mut self, session: &str) {
        let Some(finishing) = self
            .run_ahead
            .finishing
            .take_if(|finishing| finishing.fork.session == session)
        else {
            return;
        };
        // Its end counted as a failure of run-ahead already.
        self.discard_finishing_fork(&finishing, DiscardReason::Answered);
        self.answer_to_agent(&finishing.asked, finishing.turn, PlainReason::NoTurn);
    }

    /// Stop waiting: when the answer waits for its fork, the fork is discarded; when the agent
    /// switches to the fork that it waited for, the switch is undone once it ends. Either way the
    /// answer's turn is returned, for the caller to send to the agent in the pane once the stop
    /// is recorded: it waits for the agent to settle when it must.
    pub(crate) fn run_ahead_stop_waiting(&mut self) -> Option<TurnRequest> {
        const STOPPED: &str = "the reviewer stopped waiting for the fork: the answer goes to the \
                               agent in the pane";
        if let Some(finishing) = self.run_ahead.finishing.take() {
            self.drop_finishing(
                &finishing,
                (DiscardReason::StoppedWaiting, PlainReason::StoppedWaiting),
                STOPPED,
            );
            return Some(finishing.turn.request);
        }
        let switching = self
            .run_ahead
            .switching
            .as_mut()
            .filter(|switching| switching.finishing_since().is_some())?;
        switching.stopped = true;
        let (asked, request) = (switching.asked.clone(), switching.turn.request.clone());
        self.run_ahead.host.log(&format!(
            "{}: {STOPPED}, once the switch ends",
            asked.label()
        ));
        self.record_plain_after(&asked, &request.request, PlainReason::StoppedWaiting);
        Some(request)
    }

    /// Discards the fork the answer waits for, once the answer no longer waits: the reviewer
    /// cancelled it, reset the round or left it.
    pub(super) fn run_ahead_withdraw_finishing(&mut self) {
        let Some(finishing) = self.run_ahead.finishing.take_if(|finishing| {
            let request = &finishing.turn.request;
            !self.state.is_pending(&request.instance, &request.request)
        }) else {
            return;
        };
        self.drop_finishing(
            &finishing,
            (DiscardReason::Withdrawn, PlainReason::Withdrawn),
            "the answer that waited for the fork was withdrawn",
        );
    }

    /// Discards the fork `finishing` waits for, for the first of `reasons`, records the answer's
    /// path for the second, and logs `line`.
    fn drop_finishing(
        &mut self,
        finishing: &Finishing,
        (discard, path): (DiscardReason, PlainReason),
        line: &str,
    ) {
        self.run_ahead
            .host
            .log(&format!("{}: {line}", finishing.asked.label()));
        self.discard_finishing_fork(finishing, discard);
        self.record_plain_after(&finishing.asked, &finishing.turn.request.request, path);
    }

    /// Discards the fork `finishing` waits for, for `reason`.
    fn discard_finishing_fork(&mut self, finishing: &Finishing, reason: DiscardReason) {
        self.discard_forks(
            &finishing.asked.round,
            &finishing.point,
            std::slice::from_ref(&finishing.fork),
            reason,
        );
    }

    /// The answer's turn `turn` to the question `asked`, which waited for its fork, goes to the
    /// agent in the pane after all, for `reason`.
    fn answer_to_agent(&mut self, asked: &Asked, turn: SavedTurn, reason: PlainReason) {
        self.run_ahead.host.log(&format!(
            "{}: the answer goes to the agent in the pane: {reason:?}",
            asked.label()
        ));
        self.record_plain_after(asked, &turn.request.request, reason);
        if !self.run_ahead_hold(&turn) {
            self.send_turn(turn);
        }
    }
}
