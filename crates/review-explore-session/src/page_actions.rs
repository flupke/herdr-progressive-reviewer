//! The actions the Explore page offers to recover or close a round, as the pane offers them:
//! Stop waiting, Retry, Cancel answer, Reset, and the recoveries of an implementation request.
//! Each is refused as stale when the round no longer offers it, then carried out by the path the
//! pane's command takes, so that it saves the same result and sends the same prompt; the pane
//! hears of it through the events it follows.

use review_explore::{Command, DispatchState};
use review_explore_page::{CommandRefusal, Waiting};

use crate::ExploreSession;

impl ExploreSession {
    /// Stops waiting, as Stop waiting in the pane: for the start or the agent's turn `waiting`
    /// names.
    pub(crate) fn stop(&mut self, waiting: &Waiting) -> Result<(), CommandRefusal> {
        let waits = match waiting {
            Waiting::Start(start) => {
                self.state.round.is_none() && self.state.start.starting() == Some(start.as_str())
            }
            Waiting::Turn(request) => self.delivering(request),
        };
        if !waits {
            return Err(CommandRefusal::Stale);
        }
        let round = self
            .state
            .round
            .as_ref()
            .map(|round| round.exploration.instance.clone());
        self.command(Command::Cancel);
        let _ = self.events.send(ui_events::ExplorePageStopped { round });
        Ok(())
    }

    /// Whether this process sends the prompt of the agent's turn `request`, or the agent works
    /// on it.
    fn delivering(&self, request: &str) -> bool {
        self.state
            .pending
            .as_ref()
            .is_some_and(|(_, id)| id == request)
            && self.state.round.as_ref().is_some_and(|round| {
                round
                    .exploration
                    .pending_request()
                    .is_some_and(|pending| pending.request == request)
            })
    }

    /// Sends the agent's turn `request` again, as Retry in the pane, when the round waits for it
    /// after its attempt `attempt`, and the agent is not working on it. A repeat of a Retry that
    /// went through finds the turn on its way; one after a later attempt failed sends nothing.
    pub(crate) fn retry_from_page(
        &mut self,
        request: &str,
        attempt: &str,
    ) -> Result<(), CommandRefusal> {
        let round = self.saved_round()?;
        if self.delivering(request) {
            return Err(CommandRefusal::AlreadyApplied);
        }
        let retried = round
            .exploration
            .retry_request()
            .is_some_and(|retry| retry.request == request)
            && round
                .turns
                .get(request)
                .is_some_and(|delivery| delivery.attempt == attempt);
        if !retried {
            return Err(CommandRefusal::Stale);
        }
        // As the pane does: a turn saved as pending that no prompt of this process carries waits
        // for Retry.
        let mut exploration = round.exploration;
        exploration.pause_delivery();
        let request = exploration
            .retry()
            .map_err(|error| CommandRefusal::Failed(error.to_string()))?;
        self.retry(request).map_err(CommandRefusal::Failed)
    }

    /// Cancels the reviewer's latest answer `answer`, as Cancel answer in the pane, while the
    /// page offers it.
    pub(crate) fn cancel_answer_from_page(&mut self, answer: String) -> Result<(), CommandRefusal> {
        if self.cancellable().is_none_or(|latest| latest.id != answer) {
            return Err(CommandRefusal::Stale);
        }
        self.cancel_answer(answer)
            .map_err(|error| CommandRefusal::Failed(error.to_string()))
    }

    /// Closes the round `round`, as Reset in the pane, while it is the round the session shows.
    pub(crate) fn reset_round(&mut self, round: &str) -> Result<(), CommandRefusal> {
        let shown = self
            .state
            .round
            .as_ref()
            .is_some_and(|shown| shown.exploration.instance == round);
        if !shown || self.state.storage_error.is_some() {
            return Err(CommandRefusal::Stale);
        }
        self.command(Command::Reset);
        let _ = self.events.send(ui_events::ExplorePageReset {
            round: round.to_owned(),
        });
        Ok(())
    }

    /// Cancels the implementation request `delivery`, as the pane's Cancel implementation, while
    /// this process sends it.
    pub(crate) fn cancel_implementation_from_page(
        &mut self,
        delivery: &str,
    ) -> Result<(), CommandRefusal> {
        let round = self.saved_round()?;
        let sending = self.state.implementation.is_some()
            && round.implementations.get(delivery).is_some_and(|saved| {
                matches!(
                    saved.state,
                    DispatchState::Queued | DispatchState::Attempting
                )
            });
        if !sending {
            return Err(CommandRefusal::Stale);
        }
        self.command(Command::CancelImplementation);
        Ok(())
    }

    /// Sends again, as it is, the implementation request `delivery` of the conclusion of the turn
    /// `conclusion`, which an earlier process saved but did not send, or which the agent did not
    /// start on: as "Send saved implementation request" or Retry in the pane.
    pub(crate) fn resend_from_page(
        &mut self,
        conclusion: &str,
        delivery: &str,
        attempt: &str,
    ) -> Result<(), CommandRefusal> {
        let round = self.saved_round()?;
        let latest = round.latest_implementation(conclusion);
        // A repeat of a resend that went through finds the request on its way, or received.
        let resent = latest.is_some_and(|latest| {
            latest.request.delivery == delivery
                && match latest.state {
                    DispatchState::Queued | DispatchState::Attempting => {
                        self.state.implementation.is_some()
                    }
                    DispatchState::Delivered => true,
                    _ => false,
                }
        });
        if resent {
            return Err(CommandRefusal::AlreadyApplied);
        }
        let saved = latest
            .filter(|latest| {
                latest.request.delivery == delivery
                    && latest.attempt == attempt
                    && match latest.state {
                        // Queued in this process: on its way already.
                        DispatchState::Queued => self.state.implementation.is_none(),
                        DispatchState::NotStarted => true,
                        _ => false,
                    }
            })
            .map(|latest| latest.request.clone());
        let request = saved.ok_or(CommandRefusal::Stale)?;
        self.implement(request).map_err(CommandRefusal::Failed)
    }
}
