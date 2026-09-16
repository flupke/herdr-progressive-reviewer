use herdr_client::client::HerdrClient;
use herdr_client::protocol::{Agent, HerdrReader};
use review_types::ReviewUnit;

/// Access to one logical review for the selected native agent session.
#[derive(Clone)]
pub(super) struct Access {
    pub(super) token: String,
    pub(super) review_unit: ReviewUnit,
    pub(super) agent: Agent,
}

impl Access {
    pub(super) fn new(review_unit: ReviewUnit, agent: Agent) -> Result<Self, String> {
        if agent.agent_session.is_none() {
            return Err("Waiting for Herdr to identify the selected agent session".into());
        }
        Ok(Self {
            token: uuid::Uuid::new_v4().to_string(),
            review_unit,
            agent,
        })
    }

    pub(super) fn current_agent(&self, client: &HerdrClient) -> Result<Agent, String> {
        let current = client
            .get_agent(&self.agent.pane_id)
            .map_err(|error| error.to_string())?
            .ok_or("The selected agent has exited")?;
        if current.agent_session.is_none() {
            return Err("Waiting for Herdr to identify the selected agent session".into());
        }
        if !self.matches_agent(&current) {
            return Err(
                "The selected pane has changed agent sessions; retry from the reviewer to receive fresh access"
                    .into(),
            );
        }
        Ok(current)
    }

    pub(super) fn matches_agent(&self, agent: &Agent) -> bool {
        agent.pane_id == self.agent.pane_id
            && agent.workspace_id == self.agent.workspace_id
            && agent.agent_session == self.agent.agent_session
            && agent.agent == self.agent.agent
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
mod tests {
    use super::*;

    #[test]
    fn access_matches_a_resumed_session_but_not_a_different_session_or_pane() {
        let agent: Agent = serde_json::from_value(serde_json::json!({
            "pane_id": "pane", "tab_id": "tab", "workspace_id": "workspace",
            "agent": "codex", "agent_status": "idle",
            "agent_session": {"source": "herdr:codex", "agent": "codex", "kind": "id", "value": "original"},
        })).unwrap();
        let access = Access::new("review".into(), agent.clone()).unwrap();
        let mut resumed = agent.clone();
        resumed.agent_status = herdr_client::protocol::AgentStatus::Working;
        assert!(access.matches_agent(&resumed));
        resumed.agent_session.as_mut().unwrap().value = "replacement".into();
        assert!(!access.matches_agent(&resumed));
        resumed = agent.clone();
        resumed.pane_id.0 = "another-pane".into();
        assert!(!access.matches_agent(&resumed));
        resumed = agent;
        resumed.workspace_id.0 = "another-workspace".into();
        assert!(!access.matches_agent(&resumed));
        let mut unidentified = access.agent.clone();
        unidentified.agent_session = None;
        assert!(Access::new("review".into(), unidentified).is_err());
    }
}
