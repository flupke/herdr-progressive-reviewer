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
        let committed =
            self.passes
                .submit(&update.checkpoint.review_unit, &update.instance, update);
        let super::records::Submitted { applied, pass } = match committed {
            Ok(submitted) => submitted,
            Err(error) => {
                request.respond(Err(error.to_string()));
                return;
            }
        };
        let pass = if applied {
            self.apply_marks(update, &pass).unwrap_or(pass)
        } else {
            pass
        };
        self.state.pass = Some(pass.clone());
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
                .map(|applied| Response::Explore { applied });
            request.respond(result);
        });
    }

    fn authorize(&mut self, access: &str) -> eyre::Result<()> {
        eyre::ensure!(
            self.state.storage_error.is_none(),
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
