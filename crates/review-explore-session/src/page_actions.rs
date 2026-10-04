//! The actions the Explore page offers to recover or close a round, as the pane offers them:
//! Stop waiting, Retry, Cancel answer, Reset, a reply to the conclusion, and the recoveries of an
//! implementation request. Each is refused as stale when the round no longer offers it, then
//! carried out by the path the pane's command takes, so that it saves the same result and sends
//! the same prompt; the pane hears of it through the events it follows.

use review_explore::{AnswerInput, Command, DispatchState};
use review_explore_page::{CommandRefusal, PageReply};

use crate::{ExploreSession, Start};

impl ExploreSession {
    /// Stops waiting, as Stop waiting in the pane: for the start under way when `request` is
    /// `None`, else for the agent's turn `request`.
    pub(crate) fn stop(&mut self, request: Option<&str>) -> Result<(), CommandRefusal> {
        let waits = match request {
            None => self.state.round.is_none() && matches!(self.state.start, Start::Starting),
            Some(request) => self.delivering(request),
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
    /// and the agent is not working on it.
    pub(crate) fn retry_from_page(&mut self, request: &str) -> Result<(), CommandRefusal> {
        let round = self.saved_round()?;
        let retried = round
            .exploration
            .retry_request()
            .is_some_and(|retry| retry.request == request);
        if !retried || self.delivering(request) {
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

    /// Replies to the conclusion the page showed, as Reply in the pane: the turn comes from the
    /// latest saved round, while it still shows that conclusion.
    pub(crate) fn reply_from_page(&mut self, reply: PageReply) -> Result<(), CommandRefusal> {
        let round = self.saved_round()?;
        let exploration = &round.exploration;
        let concludes = exploration.pending_request().is_none()
            && exploration.retry_request().is_none()
            && exploration.conclusion_request() == Some(reply.conclusion.as_str());
        if !concludes {
            return Err(CommandRefusal::Stale);
        }
        let input = AnswerInput {
            option: None,
            text: reply.text,
            in_reply_to: Some(reply.conclusion),
            first_pick: None,
        };
        let request = exploration
            .clone()
            .request(Some(input), None)
            .map_err(|error| CommandRefusal::Failed(error.to_string()))?;
        self.deliver_turn(request, None)
            .map_err(CommandRefusal::Failed)
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
    ) -> Result<(), CommandRefusal> {
        let round = self.saved_round()?;
        let saved = round
            .latest_implementation(conclusion)
            .filter(|latest| {
                latest.request.delivery == delivery
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
