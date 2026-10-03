//! What the Explore page shows of the round the session owns.

use review_explore::ExploreRound;
use review_explore_page::RoundStage;

use crate::ExploreSession;

impl ExploreSession {
    /// Publishes the stage of the round the pane shows; the page loads itself again only when
    /// the stage changed.
    pub(crate) fn publish_page(&self) {
        self.page.publish(self.page_stage());
    }

    fn page_stage(&self) -> RoundStage {
        let Some(round) = &self.state.round else {
            return RoundStage::NoRound;
        };
        let exploration = &round.exploration;
        if let Some(request) = exploration.pending_request() {
            // A turn saved as pending that no prompt of this process carries, after a
            // reopening or Stop waiting, waits for Retry.
            let delivering = self.state.pending.as_ref().is_some_and(|(instance, id)| {
                *instance == request.instance && *id == request.request
            });
            return if delivering {
                RoundStage::AgentWorking
            } else {
                RoundStage::Interrupted
            };
        }
        if exploration.retry_request().is_some() {
            return RoundStage::Interrupted;
        }
        latest_turn(round)
    }
}

/// The question or conclusion the agent's latest turn posted.
fn latest_turn(round: &ExploreRound) -> RoundStage {
    let exploration = &round.exploration;
    let Some(turn) = exploration.conversation.last() else {
        return RoundStage::Interrupted;
    };
    if let Some(conclusion) = &turn.update.conclusion {
        return RoundStage::Conclusion(Box::new(conclusion.clone()));
    }
    match &turn.update.next {
        Some(question) => RoundStage::Question {
            number: exploration.questions.len(),
            question: Box::new(question.clone()),
        },
        None => RoundStage::Interrupted,
    }
}
