//! Agent MCP calls to the Explore tools, authorized by the current access value.

use std::sync::Arc;

use review_explore::ExploreRound;
use review_mcp::{Operation, Request, Response};

use crate::ExploreSession;

impl ExploreSession {
    pub(crate) fn submission(&mut self, request: Request) {
        // A fork's calls never reach the round.
        let Some(request) = self.run_ahead_call(request) else {
            return;
        };
        let agent = match self.authorize(&request.access) {
            Ok(agent) => agent,
            Err(error) => {
                self.refuse(request, &error);
                return;
            }
        };
        if let (Some(hooks), Some(session)) = (&self.hooks, &request.agent_session) {
            hooks.name_session(&agent.pane_id, session);
        }
        if let Operation::ReviseQuestion(revision) = &request.operation {
            let revision = (**revision).clone();
            self.revise(request, &revision);
            return;
        }
        let update = match update_of(&request.operation) {
            Ok(update) => update,
            Err(error) => {
                request.respond(Err(error));
                return;
            }
        };
        self.submit(request, &update);
    }

    /// Answers `request`, whose access [`Self::authorize`] refused for `error`.
    fn refuse(&self, request: Request, error: &eyre::Report) {
        let revising = matches!(request.operation, Operation::ReviseQuestion(_));
        let other_access = revising
            && self.state.storage_error.is_none()
            && !self.state.historical
            && self.state.access != request.access;
        request.respond(Err(if other_access {
            // Each answer the reviewer sends renews the access.
            "This is not the Explore review access of the round now. Use the one of your \
             latest Explore prompt, not the comments wakeup's. If you did, the round moved \
             on since that prompt: the reviewer answered, cancelled an answer or reopened. \
             Ask the new version in your next Explore turn"
                .into()
        } else {
            error.to_string()
        }));
    }

    fn submit(&mut self, request: Request, update: &review_explore::InterviewUpdate) {
        let (applied, round) = match self.commit(update) {
            Ok(committed) => committed,
            Err(error) => {
                request.respond(Err(error));
                return;
            }
        };
        let shown_as = update
            .next
            .as_ref()
            .and_then(|question| round.exploration.question_number(question))
            .map(Into::into);
        self.acknowledge(request, applied, round, shown_as);
    }

    /// Saves the agent's `revision` of the open question, and shows it.
    fn revise(&mut self, request: Request, revision: &review_explore::QuestionRevision) {
        let committed = self.revised(revision);
        let (applied, round) = match committed {
            Ok(committed) => committed,
            Err(error) => {
                request.respond(Err(error));
                return;
            }
        };
        let shown_as = round
            .exploration
            .question_number(&revision.question)
            .map(Into::into);
        self.acknowledge(request, applied, round, shown_as);
    }

    /// Saves `revision` in the session's round; whether it changed the round, and the round
    /// after it.
    fn revised(
        &mut self,
        revision: &review_explore::QuestionRevision,
    ) -> Result<(bool, ExploreRound), String> {
        if self
            .state
            .round
            .as_ref()
            .is_none_or(|round| round.exploration.instance != revision.instance)
        {
            return Err("Explore revision belongs to another instance".into());
        }
        let super::records::Submitted { applied, round } = self
            .rounds
            .revise(
                &revision.checkpoint.review_unit,
                &revision.instance,
                revision,
            )
            .map_err(|error| error.to_string())?;
        self.state.round = Some(round.clone());
        Ok((applied, round))
    }

    /// Shows the round `round` an accepted call left, and answers `request` once the reviewer
    /// applied it.
    fn acknowledge(
        &self,
        request: Request,
        applied: bool,
        round: ExploreRound,
        shown_as: Option<review_types::QuestionNumber>,
    ) {
        let (response, received) = std::sync::mpsc::channel();
        if self
            .events
            .send(ui_events::ExploreCommitted {
                implementations: self.implementation_progress(&round),
                round: Arc::new(round),
                applied,
                response,
            })
            .is_err()
        {
            request.respond(Err(
                "The reviewer is closed; the accepted response was saved".into(),
            ));
            return;
        }
        std::thread::spawn(move || {
            let result = received
                .recv_timeout(std::time::Duration::from_secs(15))
                .map_err(|_| {
                    "The reviewer did not acknowledge Explore; retry the identical payload"
                        .to_owned()
                })
                .and_then(|result| result)
                .map(|applied| Response::Explore { applied, shown_as });
            request.respond(result);
        });
    }

    /// Saves the agent's turn `update` in the session's round, with the marks of a
    /// conclusion; returns whether the turn changed the round, and the round after it.
    pub(crate) fn commit(
        &mut self,
        update: &review_explore::InterviewUpdate,
    ) -> Result<(bool, ExploreRound), String> {
        if self
            .state
            .round
            .as_ref()
            .is_none_or(|round| round.exploration.instance != update.instance)
        {
            return Err("Explore response belongs to another instance".into());
        }
        let super::records::Submitted { applied, round } = self
            .rounds
            .submit(&update.checkpoint.review_unit, &update.instance, update)
            .map_err(|error| error.to_string())?;
        let round = if applied {
            self.apply_conclusion_marks(update, &round).unwrap_or(round)
        } else {
            round
        };
        self.state.round = Some(round.clone());
        Ok((applied, round))
    }

    /// Checks that `access` is the round's now, and returns the round's agent.
    fn authorize(&mut self, access: &str) -> eyre::Result<herdr_client::protocol::Agent> {
        eyre::ensure!(
            self.state.storage_error.is_none(),
            "Explore storage is unavailable: {}",
            self.state.storage_error.as_deref().unwrap_or_default()
        );
        eyre::ensure!(
            !access.is_empty() && self.state.access == access && !self.state.historical,
            "Obsolete Explore access: the reviewer reopened, retried, cancelled or reset this turn. Its next prompt carries fresh access; do not resubmit until then"
        );
        let round = self
            .state
            .round
            .as_ref()
            .ok_or_else(|| eyre::eyre!("No Explore round"))?;
        self.state.round = Some(
            self.rounds
                .round(
                    &round.exploration.comparison.checkpoint.review_unit,
                    &round.exploration.instance,
                )?
                .ok_or_else(|| {
                    eyre::eyre!(
                        "Saved Explore round is missing; restore its state before continuing"
                    )
                })?,
        );
        let agent = self
            .active_agent()?
            .current(&*self.agents)
            .map_err(eyre::Report::msg)?
            .ok_or_else(|| eyre::eyre!("Waiting for the native agent conversation identity"))?;
        let session = review_explore::ConversationBinding::from_agent(&agent);
        let round = self.state.round.as_ref().expect("active round");
        if round.last_agent_session != session {
            self.rounds.update(
                &round.exploration.comparison.checkpoint.review_unit,
                &round.exploration.instance,
                |round| {
                    round.last_agent_session = session;
                    Ok(())
                },
            )?;
        }
        Ok(agent)
    }
}

/// The agent's turn an Explore tool call carries, or why it carries none.
pub(crate) fn update_of(operation: &Operation) -> Result<review_explore::InterviewUpdate, String> {
    match operation {
        Operation::SubmitQuestion(update)
            if update.next.is_some() && update.conclusion.is_none() =>
        {
            Ok((**update).clone())
        }
        Operation::SubmitConclusion(conclusion) => Ok((**conclusion).clone().into_update()),
        _ => Err("submit_question requires one question; use submit_conclusion to finish".into()),
    }
}
