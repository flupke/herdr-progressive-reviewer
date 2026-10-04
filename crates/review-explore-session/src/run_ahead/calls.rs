//! A fork's calls to the reviewer: its access value lets it submit a question or a conclusion
//! for the request it was forked for, and nothing else. What it submits is checked as the
//! agent's turns are, then kept for its choice and shown to nobody.

use review_explore::InterviewUpdate;
use review_mcp::{Operation, Request, Response};

use super::TakenFork;
use crate::ExploreSession;

/// What a discarded fork hears from every call.
const DISCARDED: &str = "This turn is no longer needed: the reviewer's round moved on. Do not \
                         submit again; end your turn now.";

impl ExploreSession {
    /// Answers the call `request` when a fork made it; hands back any other call.
    pub(crate) fn run_ahead_call(&mut self, request: Request) -> Option<Request> {
        if self.run_ahead.discarded.contains(&request.access) {
            request.respond(Err(DISCARDED.into()));
            return None;
        }
        let Some(fork) = self.run_ahead.fork_with_access(&request.access).cloned() else {
            return Some(request);
        };
        let result = crate::submission::update_of(&request.operation)
            .and_then(|update| self.keep_fork_turn(&fork, update, &request.operation));
        request.respond(result);
        None
    }

    /// Checks the turn `update` that `fork` submitted, and keeps it for its choice.
    fn keep_fork_turn(
        &mut self,
        fork: &TakenFork,
        update: InterviewUpdate,
        operation: &Operation,
    ) -> Result<Response, String> {
        if update.instance != fork.request.instance || update.request != fork.request.request {
            return Err(format!(
                "This turn is for the request {} of the round {}; submit for those only",
                fork.request.request, fork.request.instance
            ));
        }
        if let Some(kept) = &fork.kept {
            return if *kept == update {
                Ok(Response::Explore {
                    applied: false,
                    shown_as: None,
                })
            } else {
                Err("This turn is already submitted; end your turn now".into())
            };
        }
        let round = self.state.round.as_ref().ok_or("No Explore round")?;
        // The round as it would be had the reviewer picked the fork's choice.
        let mut answered = round.clone();
        answered
            .post(&fork.request)
            .and_then(|_| answered.submit(&update))
            .map_err(|error| error.to_string())?;
        let shown_as = update
            .next
            .as_ref()
            .and_then(|question| answered.exploration.question_number(question))
            .map(Into::into);
        let round = self
            .run_ahead
            .armed
            .as_ref()
            .expect("the fork's question waits")
            .round
            .clone();
        let kept = Box::new(update.clone());
        self.update_forks(&round, |forks| {
            if let Some(record) = forks.fork_mut(&fork.session) {
                record.turn = Some(kept);
            }
        })
        .map_err(|error| error.to_string())?;
        let kind = if matches!(operation, Operation::SubmitConclusion(_)) {
            "a conclusion"
        } else {
            "a question"
        };
        self.run_ahead
            .log(&format!("the fork for {} submitted {kind}", fork.choice));
        if let Some(taken) = self.run_ahead.fork_mut(&fork.session) {
            taken.kept = Some(update);
        }
        Ok(Response::Explore {
            applied: true,
            shown_as,
        })
    }
}
