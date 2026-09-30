//! Agent MCP calls to the Explore tools, authorized by the current access value.

use std::sync::Arc;

use review_mcp::{Operation, Request, Response};

use crate::ExploreSession;

impl ExploreSession {
    pub(crate) fn submission(&mut self, request: Request) {
        if let Err(error) = self.authorize(&request.access) {
            request.respond(Err(error.to_string()));
            return;
        }
        if let Operation::GetCoverageGaps(query) = &request.operation {
            let result = self.coverage_gap_page(query);
            request.respond(
                result
                    .map(|page| Response::CoverageGaps(Box::new(page)))
                    .map_err(|error| error.to_string()),
            );
            return;
        }
        let update = match &request.operation {
            Operation::SubmitQuestion(update)
                if update.next.is_some() && update.conclusion.is_none() =>
            {
                (**update).clone()
            }
            Operation::SubmitConclusion(conclusion) => (**conclusion).clone().into_update(),
            _ => {
                request.respond(Err(
                    "submit_question requires one question; use submit_conclusion to finish".into(),
                ));
                return;
            }
        };
        self.submit(request, &update);
    }

    fn submit(&mut self, request: Request, update: &review_explore::InterviewUpdate) {
        if self
            .state
            .pass
            .as_ref()
            .is_none_or(|pass| pass.exploration.instance != update.instance)
        {
            request.respond(Err("Explore response belongs to another instance".into()));
            return;
        }
        let committed = self.passes.submit(
            &update.checkpoint.review_unit,
            &update.instance,
            update,
            self.exclusion.is_enabled(),
        );
        let (applied, pass, coverage) = match committed {
            Ok(result) => result,
            Err(error) => {
                request.respond(Err(error.to_string()));
                return;
            }
        };
        self.state.pass = Some(pass.clone());
        if pass
            .completion
            .as_ref()
            .is_some_and(|completion| completion.completed)
        {
            self.state.storage_error = None;
        }
        let (response, received) = std::sync::mpsc::channel();
        if self
            .events
            .send(ui_events::ExploreCommitted {
                pass: Arc::new(pass),
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
                .map(|applied| Response::Explore {
                    applied,
                    coverage: Box::new(coverage),
                });
            request.respond(result);
        });
    }

    fn coverage_gap_page(
        &self,
        query: &review_explore::GapQuery,
    ) -> eyre::Result<review_explore::GapPage> {
        let pass = self
            .state
            .pass
            .as_ref()
            .ok_or_else(|| eyre::eyre!("No Explore pass"))?;
        eyre::ensure!(
            pass.exploration.instance == query.instance
                && pass.exploration.comparison.checkpoint == query.checkpoint,
            "Gap request belongs to another pass or checkpoint"
        );
        eyre::ensure!(
            pass.coverage.revision() == query.revision && self.exclusion.mode() == query.mode,
            "Coverage revision or Jev policy changed; use the latest current feedback"
        );
        let limit = query.limit.unwrap_or(64);
        eyre::ensure!(
            (1..=128).contains(&limit),
            "Gap page limit must be 1 to 128"
        );
        Ok(pass.coverage.gap_page(
            &pass.exploration.comparison,
            &pass.pending_questions(),
            self.exclusion.is_enabled(),
            query.path_prefix.as_deref(),
            query.cursor.unwrap_or(0),
            limit,
        ))
    }

    fn authorize(&mut self, access: &str) -> eyre::Result<()> {
        eyre::ensure!(
            self.state.storage_error.is_none()
                || self
                    .state
                    .pass
                    .as_ref()
                    .and_then(|pass| pass.completion.as_ref())
                    .is_some_and(|completion| !completion.completed),
            "Explore storage is unavailable: {}",
            self.state.storage_error.as_deref().unwrap_or_default()
        );
        eyre::ensure!(
            !access.is_empty() && self.state.access == access && !self.state.historical,
            "Obsolete Explore access; retry the interrupted turn from the reviewer for fresh access"
        );
        let pass = self
            .state
            .pass
            .as_ref()
            .ok_or_else(|| eyre::eyre!("No Explore pass"))?;
        self.state.pass = Some(
            self.passes
                .pass(
                    &pass.exploration.comparison.checkpoint.review_unit,
                    &pass.exploration.instance,
                )?
                .ok_or_else(|| {
                    eyre::eyre!(
                        "Saved Explore pass is missing; restore its state before continuing"
                    )
                })?,
        );
        let agent = self
            .active_agent()?
            .current(&*self.agents)
            .map_err(eyre::Report::msg)?
            .ok_or_else(|| eyre::eyre!("Waiting for the native agent conversation identity"))?;
        let session = review_explore::ConversationBinding::from_agent(&agent);
        let pass = self.state.pass.as_ref().expect("active pass");
        if pass.last_agent_session != session {
            self.passes.update(
                &pass.exploration.comparison.checkpoint.review_unit,
                &pass.exploration.instance,
                |pass| {
                    pass.last_agent_session = session;
                    Ok(())
                },
            )?;
        }
        Ok(())
    }
}
