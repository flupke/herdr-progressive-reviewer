//! What the Explore page shows of the round the session owns, and the commands the reviewer
//! sends from it.

use std::path::Path;
use std::sync::Arc;

use review_explore::{Comparison, Exploration, ExploreRound, Question};
use review_explore_citations::{Citation, CodeColors};
use review_explore_page::{
    CommandRefusal, CommandReply, PageAnswer, PageCommand, PublishedRound, QuestionMarks,
    RoundStage,
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
        }
    }

    /// Answers the question the page showed, as the pane would answer it. The turn comes from
    /// the latest saved round, so the page never answers from a stale copy, and a question
    /// answered already, from the pane or from another page, is refused.
    fn answer_from_page(&mut self, answer: PageAnswer) -> Result<(), CommandRefusal> {
        let failed = |error: &dyn std::fmt::Display| CommandRefusal::Failed(error.to_string());
        let shown = self.state.round.as_ref().ok_or(CommandRefusal::Stale)?;
        let round = self
            .rounds
            .round(
                &shown.exploration.comparison.checkpoint.review_unit,
                &shown.exploration.instance,
            )
            .map_err(|error| failed(&error))?
            .ok_or(CommandRefusal::Stale)?;
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
        latest_turn(round, &mut self.citations, self.repository.root())
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

/// The question or conclusion the agent's latest turn posted.
fn latest_turn(round: &ExploreRound, citations: &mut PageCitations, root: &Path) -> RoundStage {
    let exploration = &round.exploration;
    let Some(turn) = exploration.conversation.last() else {
        return RoundStage::Interrupted { failure: None };
    };
    if let Some(conclusion) = &turn.update.conclusion {
        return RoundStage::Conclusion(Box::new(conclusion.clone()));
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
