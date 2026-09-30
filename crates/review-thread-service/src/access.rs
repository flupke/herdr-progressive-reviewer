use std::sync::{Arc, Mutex, PoisonError};

use herdr_client::protocol::{Agent, AgentPort};
use review_types::ReviewUnit;

use crate::agent_identity::{AgentIdentity, IdentityError, IdentityRules, Verdict};

/// Access to one logical review for the selected agent process or native session.
#[derive(Clone)]
pub(super) struct Access {
    pub(super) token: String,
    pub(super) review_unit: ReviewUnit,
    pub(super) agent: Agent,
    /// Clones share one identity, so a session adopted through any clone binds them all.
    identity: Arc<Mutex<AgentIdentity>>,
}

impl Access {
    pub(super) fn new(
        review_unit: ReviewUnit,
        agent: Agent,
        port: &dyn AgentPort,
    ) -> Result<Self, String> {
        let identity = AgentIdentity::new(&agent, IdentityRules::REVIEW_ACCESS)
            .bind_process_without_session(port)
            .map_err(Self::identity_error)?;
        Ok(Self {
            token: uuid::Uuid::new_v4().to_string(),
            review_unit,
            agent,
            identity: Arc::new(Mutex::new(identity)),
        })
    }

    pub(super) fn current_agent(&self, port: &dyn AgentPort) -> Result<Agent, String> {
        let current = port
            .get_agent(&self.agent.pane_id)
            .map_err(|error| error.to_string())?
            .ok_or("The selected agent has exited")?;
        if !self.matches_agent(port, &current)? {
            return Err(
                "The selected pane has changed agent processes or sessions; retry from the reviewer to receive fresh access"
                    .into(),
            );
        }
        Ok(current)
    }

    /// A missing native session does not match: access values are bearer tokens.
    pub(super) fn matches_agent(
        &self,
        port: &dyn AgentPort,
        agent: &Agent,
    ) -> Result<bool, String> {
        let verdict = self
            .identity
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .check(port, agent)
            .map_err(Self::identity_error)?;
        Ok(verdict == Verdict::Same)
    }

    fn identity_error(error: IdentityError) -> String {
        error.into_message("Herdr did not identify the selected agent process")
    }

    pub(super) fn prompt(&self) -> String {
        format!(
            "There are review comments for you. Use the herdr_reviewer MCP tools with review access value `{}`. Call get_new_messages to retrieve all pending threads with their full conversations and original code context. Address the feedback and append your responses with the reply tool, copying in_reply_to from each fetched thread. Unresolved comments remain pending until a reply succeeds. An agent reply is a thread message and may address several comments. Before finishing, call get_new_messages again and address any comments that arrived while you worked. Do not resolve threads. If a reply call fails, retry using its exact same message_id, text and in_reply_to. If the reviewer is closed or MCP is unavailable, report that and stop.\n\nLogical review: {}",
            self.token,
            self.review_unit.as_str(),
        )
    }
}

#[cfg(test)]
#[path = "access.tests.rs"]
mod tests;
