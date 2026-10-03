//! What the Explore page shows of the round the session owns.

use std::path::Path;
use std::sync::Arc;

use review_explore::{Comparison, ExploreRound, Question};
use review_explore_citations::{Citation, CodeColors};
use review_explore_page::RoundStage;
use review_source::ReviewCheckpoint;

use crate::ExploreSession;

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
        let round = self
            .state
            .round
            .as_ref()
            .map(|round| round.exploration.instance.as_str());
        self.page.publish(round, stage);
    }

    fn page_stage(&mut self) -> RoundStage {
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
        latest_turn(round, &mut self.citations, self.repository.root())
    }
}

/// The question or conclusion the agent's latest turn posted.
fn latest_turn(round: &ExploreRound, citations: &mut PageCitations, root: &Path) -> RoundStage {
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
            citations: citations.of(question, &exploration.comparison, root),
        },
        None => RoundStage::Interrupted,
    }
}
