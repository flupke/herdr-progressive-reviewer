//! What the Explore page shows of the round the session owns, and the commands the reviewer
//! sends from it.

use std::path::Path;
use std::sync::Arc;

use review_explore::{
    Comparison, DispatchState, Exploration, ExploreRound, ImplementationDelivery, Question,
};
use review_explore_citations::{Citation, CodeColors};
use review_explore_page::{
    CommandRefusal, CommandReply, ImplementationState, PageAnswer, PageCommand, PageImplement,
    PageImplementation, PublishedRound, QuestionMarks, RoundStage,
};
use review_source::ReviewCheckpoint;

use crate::{ExploreSession, Start};

/// The citations of the question the page shows, found in the change and colored once per
/// question rather than on every input the session handles.
#[derive(Default)]
pub(crate) struct PageCitations {
    colors: CodeColors,
    shown: Option<Shown>,
}

/// The question whose citations the page shows, and the checkpoint they were found at.
struct Shown {
    checkpoint: ReviewCheckpoint,
    question: Question,
    citations: Arc<[Citation]>,
}

impl PageCitations {
    /// The citations of `question`, with the lines `comparison` has for them in `root`.
    fn of(&mut self, question: &Question, comparison: &Comparison, root: &Path) -> Arc<[Citation]> {
        if let Some(shown) = &self.shown
            && shown.checkpoint == comparison.checkpoint
            && shown.question == *question
        {
            return shown.citations.clone();
        }
        let citations: Arc<[Citation]> = question
            .evidence
            .iter()
            .map(|evidence| {
                let lines = comparison.cited_lines(&evidence.location, root);
                self.colors.cite(evidence.clone(), lines)
            })
            .collect();
        self.shown = Some(Shown {
            checkpoint: comparison.checkpoint.clone(),
            question: question.clone(),
            citations: citations.clone(),
        });
        citations
    }
}

impl ExploreSession {
    /// Publishes the stage of the round the pane shows; the page loads itself again only when
    /// the stage changed.
    pub(crate) fn publish_page(&mut self) {
        let stage = self.page_stage();
        let round = self.state.round.as_ref().map(|round| PublishedRound {
            id: &round.exploration.instance,
            design: round.exploration.design(),
        });
        self.page.publish(round, stage);
    }

    /// Carries out `command` from the page, or refuses it, and replies.
    pub(crate) fn page_command(&mut self, command: PageCommand, reply: CommandReply) {
        match command {
            PageCommand::Answer(answer) => {
                let result = self.answer_from_page(answer);
                // The page loads itself again once it has the reply: it shows the new stage.
                self.publish_page();
                reply.send(result);
            }
            PageCommand::DiagramFailed(error) => {
                let result = self.diagram_failed(error);
                self.publish_page();
                reply.send(result);
            }
            // A reviewer's worker calls `start_from_page` itself, so that Jev marks before the
            // kickoff: this sends the kickoff at once.
            PageCommand::Start { challenger } => {
                if let Some(kickoff) = self.start_from_page(challenger, reply) {
                    let _ = self.deliver_turn(kickoff, None);
                }
            }
            PageCommand::Implement(implement) => {
                let result = self.implement_from_page(implement);
                self.publish_page();
                reply.send(result);
            }
        }
    }

    /// The latest saved copy of the round the pane shows, so that the page never acts on a
    /// stale copy.
    fn saved_round(&self) -> Result<ExploreRound, CommandRefusal> {
        let shown = self.state.round.as_ref().ok_or(CommandRefusal::Stale)?;
        self.rounds
            .round(
                &shown.exploration.comparison.checkpoint.review_unit,
                &shown.exploration.instance,
            )
            .map_err(|error| CommandRefusal::Failed(error.to_string()))?
            .ok_or(CommandRefusal::Stale)
    }

    /// Answers the question the page showed, as the pane would answer it. The turn comes from
    /// the latest saved round, so the page never answers from a stale copy, and a question
    /// answered already, from the pane or from another page, is refused.
    fn answer_from_page(&mut self, answer: PageAnswer) -> Result<(), CommandRefusal> {
        let failed = |error: &dyn std::fmt::Display| CommandRefusal::Failed(error.to_string());
        let round = self.saved_round()?;
        let question = waiting_question(&round.exploration)
            .filter(|question| question.is_version(&answer.question, answer.version))
            .cloned()
            .ok_or(CommandRefusal::Stale)?;
        let request = round
            .exploration
            .clone()
            .request(Some(answer.input), Some(&question))
            .map_err(|error| failed(&error))?;
        self.deliver_turn(request, None)
            .map_err(CommandRefusal::Failed)
    }

    /// Implements the conclusion the page showed, as the pane would: the request comes from
    /// the latest saved round, and an empty list is refused. The page may send one only while
    /// the conclusion has no request the agent may have received, and only in place of the request
    /// it showed, so that a repeated or stale Implement cannot start a second implementation.
    fn implement_from_page(&mut self, implement: PageImplement) -> Result<(), CommandRefusal> {
        let round = self.saved_round()?;
        if round.exploration.conclusion_request() != Some(implement.conclusion.as_str()) {
            return Err(CommandRefusal::Stale);
        }
        let offered = match round.latest_implementation(&implement.conclusion) {
            None => implement.replaces.is_none(),
            Some(latest) => {
                latest.state.undelivered()
                    && implement.replaces.as_deref() == Some(latest.request.delivery.as_str())
            }
        };
        if !offered {
            return Err(CommandRefusal::Stale);
        }
        let request = round
            .exploration
            .implementation(implement.text)
            .map_err(|error| CommandRefusal::Failed(error.to_string()))?;
        self.implement(request).map_err(CommandRefusal::Failed)
    }

    fn page_stage(&mut self) -> RoundStage {
        let Some(round) = &self.state.round else {
            return match &self.state.start {
                Start::Idle => RoundStage::NoRound,
                Start::Starting => RoundStage::Starting,
                Start::Failed(failure) => RoundStage::StartFailed {
                    failure: failure.clone(),
                },
            };
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
                RoundStage::Interrupted { failure: None }
            };
        }
        if let Some(retry) = exploration.retry_request() {
            return RoundStage::Interrupted {
                failure: retry.response_error.clone(),
            };
        }
        let sending = self.state.implementation.is_some();
        latest_turn(round, sending, &mut self.citations, self.repository.root())
    }
}

/// The question the round waits for an answer to: the one the agent's latest turn posted,
/// unless a turn is pending or waits for Retry.
fn waiting_question(exploration: &Exploration) -> Option<&Question> {
    if exploration.pending_request().is_some() || exploration.retry_request().is_some() {
        return None;
    }
    exploration.conversation.last()?.update.next.as_ref()
}

/// The question or conclusion the agent's latest turn posted. `sending` tells whether this
/// process sends an implementation request.
fn latest_turn(
    round: &ExploreRound,
    sending: bool,
    citations: &mut PageCitations,
    root: &Path,
) -> RoundStage {
    let exploration = &round.exploration;
    let Some(turn) = exploration.conversation.last() else {
        return RoundStage::Interrupted { failure: None };
    };
    if let Some(conclusion) = &turn.update.conclusion {
        let request = &turn.update.request;
        return RoundStage::Conclusion {
            request: request.clone(),
            conclusion: Box::new(conclusion.clone()),
            implementation: round
                .latest_implementation(request)
                .map(|delivery| page_implementation(delivery, sending)),
        };
    }
    match &turn.update.next {
        Some(question) => RoundStage::Question {
            number: exploration.questions.len(),
            question: Box::new(question.clone()),
            citations: citations.of(question, &exploration.comparison, root),
            marks: QuestionMarks::requested(&turn.update),
        },
        None => RoundStage::Interrupted { failure: None },
    }
}

/// An implementation request as the page shows it. A request saved as queued or attempting is
/// on its way when this process sends it; otherwise an earlier process left it paused or with
/// an unknown outcome.
fn page_implementation(delivery: &ImplementationDelivery, sending: bool) -> PageImplementation {
    let state = match &delivery.state {
        DispatchState::Queued | DispatchState::Attempting if sending => {
            ImplementationState::Sending
        }
        DispatchState::Queued => ImplementationState::Paused,
        DispatchState::Attempting | DispatchState::Unknown => ImplementationState::Unknown,
        DispatchState::Delivered => ImplementationState::Sent,
        DispatchState::NotSent(reason) => ImplementationState::NotSent(reason.clone()),
        DispatchState::Cancelled => ImplementationState::Cancelled,
    };
    PageImplementation {
        delivery: delivery.request.delivery.clone(),
        text: delivery.request.text.clone(),
        state,
    }
}
