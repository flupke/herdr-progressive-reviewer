//! Cancelling the reviewer's latest answer: the interview returns to the
//! question it answered, as if the answer had never been sent.

use crate::{Exploration, ExplorePass, ReviewerAnswer, TurnMarks};

/// An answer the reviewer took back, and the agent turn it discarded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CancelledAnswer {
    pub answer: ReviewerAnswer,
    /// The request that carried the answer to the agent.
    pub request: Option<String>,
    /// The review marks the agent's turn after the answer changed.
    pub marks: Option<TurnMarks>,
}

impl Exploration {
    /// Forget the latest answer and the agent's turn after it, and tell the
    /// agent with its next request.
    fn cancel_answer(&mut self, id: &str) -> eyre::Result<CancelledAnswer> {
        eyre::ensure!(
            self.answers.last().is_some_and(|answer| answer.id == id),
            "Only the latest answer can be cancelled"
        );
        let answer = self.answers.pop().expect("latest answer");
        let carries = |request: &&crate::TurnRequest| {
            request
                .answer
                .as_ref()
                .is_some_and(|carried| carried.id == id)
        };
        let pending = self
            .outstanding
            .as_ref()
            .filter(carries)
            .or(self.retry.as_ref().filter(carries))
            .cloned();
        if pending.is_some() {
            self.outstanding = None;
            self.retry = None;
        }
        let answered = self
            .conversation
            .iter()
            .position(|turn| turn.answer.as_deref() == Some(id));
        let request = match answered {
            Some(index) => Some(self.conversation.remove(index).update.request),
            None => pending.as_ref().map(|request| request.request.clone()),
        };
        if answered.is_some() {
            self.replay();
        }
        // Answers cancelled earlier that only this request would have told the agent about.
        if let Some(pending) = pending {
            self.cancelled.splice(0..0, pending.cancelled);
        }
        self.cancelled.push(answer.id.clone());
        Ok(CancelledAnswer {
            answer,
            request,
            marks: None,
        })
    }

    /// Rebuild what the conversation's turns decided, after one was removed.
    fn replay(&mut self) {
        self.topics.clear();
        self.interpretations.clear();
        self.questions.clear();
        self.limitations.clear();
        self.findings.clear();
        self.conclusion = None;
        for turn in self.conversation.clone() {
            let answer = turn
                .answer
                .as_ref()
                .and_then(|id| self.answers.iter().find(|answer| &answer.id == id))
                .cloned();
            self.absorb(turn.update, answer.as_ref());
        }
    }
}

impl ExplorePass {
    /// Cancel the latest answer, unless implementation was already requested.
    pub fn cancel_answer(&mut self, id: &str) -> eyre::Result<CancelledAnswer> {
        eyre::ensure!(
            self.implementations.is_empty(),
            "Implementation was requested; answers can no longer be cancelled"
        );
        let mut cancelled = self.exploration.cancel_answer(id)?;
        // Every delivery that carried the answer, including interrupted ones.
        let mut requests: Vec<String> = self
            .turns
            .iter()
            .filter(|(_, turn)| turn.request.answer.as_ref().is_some_and(|a| a.id == id))
            .map(|(request, _)| request.clone())
            .collect();
        requests.extend(cancelled.request.clone());
        for request in &requests {
            self.turns.remove(request);
            if let Some(marks) = self.marks.remove(request) {
                cancelled.marks = Some(marks);
            }
            self.exploration.agent_elapsed_ms.remove(request);
            if self
                .completion
                .as_ref()
                .is_some_and(|completion| &completion.request == request)
            {
                self.completion = None;
            }
        }
        Ok(cancelled)
    }
}
