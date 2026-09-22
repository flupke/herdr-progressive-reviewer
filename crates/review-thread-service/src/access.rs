use std::sync::{Arc, OnceLock};

use herdr_client::client::HerdrClient;
use herdr_client::protocol::{Agent, AgentSession, HerdrReader};
use review_types::ReviewUnit;

#[derive(Clone)]
enum AgentIdentity {
    NativeSession(AgentSession),
    ForegroundProcessGroup {
        group: u32,
        native_session: Arc<OnceLock<AgentSession>>,
    },
}

impl AgentIdentity {
    fn matches_session(&self, current: Option<&AgentSession>) -> bool {
        match self {
            Self::NativeSession(expected) => current == Some(expected),
            Self::ForegroundProcessGroup { native_session, .. } => {
                match (current, native_session.get()) {
                    (Some(current), Some(expected)) => current == expected,
                    (Some(current), None) => {
                        native_session.get_or_init(|| current.clone()) == current
                    }
                    (None, Some(_)) => false,
                    (None, None) => true,
                }
            }
        }
    }
}

/// Access to one logical review for the selected agent process or native session.
#[derive(Clone)]
pub(super) struct Access {
    pub(super) token: String,
    pub(super) review_unit: ReviewUnit,
    pub(super) agent: Agent,
    identity: AgentIdentity,
}

impl Access {
    pub(super) fn new(
        review_unit: ReviewUnit,
        agent: Agent,
        client: &HerdrClient,
    ) -> Result<Self, String> {
        let identity = if let Some(session) = &agent.agent_session {
            AgentIdentity::NativeSession(session.clone())
        } else {
            AgentIdentity::ForegroundProcessGroup {
                group: Self::process_group(client, &agent)?,
                native_session: Arc::new(OnceLock::new()),
            }
        };
        Ok(Self {
            token: uuid::Uuid::new_v4().to_string(),
            review_unit,
            agent,
            identity,
        })
    }

    pub(super) fn current_agent(&self, client: &HerdrClient) -> Result<Agent, String> {
        let current = client
            .get_agent(&self.agent.pane_id)
            .map_err(|error| error.to_string())?
            .ok_or("The selected agent has exited")?;
        if !self.matches_agent(client, &current)? {
            return Err(
                "The selected pane has changed agent processes or sessions; retry from the reviewer to receive fresh access"
                    .into(),
            );
        }
        Ok(current)
    }

    pub(super) fn matches_agent(
        &self,
        client: &HerdrClient,
        agent: &Agent,
    ) -> Result<bool, String> {
        if agent.pane_id != self.agent.pane_id
            || agent.workspace_id != self.agent.workspace_id
            || agent.agent != self.agent.agent
        {
            return Ok(false);
        }
        if let AgentIdentity::ForegroundProcessGroup { group, .. } = &self.identity
            && Self::process_group(client, agent)? != *group
        {
            return Ok(false);
        }
        Ok(self.identity.matches_session(agent.agent_session.as_ref()))
    }

    fn process_group(client: &HerdrClient, agent: &Agent) -> Result<u32, String> {
        client
            .pane_process_info(&agent.pane_id)
            .map_err(|error| error.to_string())?
            .foreground_process_group_id
            .filter(|group| *group != 0)
            .ok_or("Herdr did not identify the selected agent process".into())
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
        let client = HerdrClient::new(
            "/nonexistent/reviewer-test.sock".into(),
            "reviewer-test".into(),
            "/nonexistent".into(),
        );
        let agent: Agent = serde_json::from_value(serde_json::json!({
            "pane_id": "pane", "tab_id": "tab", "workspace_id": "workspace",
            "agent": "codex", "agent_status": "idle",
            "agent_session": {"source": "herdr:codex", "agent": "codex", "kind": "id", "value": "original"},
        })).unwrap();
        let access = Access::new("review".into(), agent.clone(), &client).unwrap();
        let mut resumed = agent.clone();
        resumed.agent_status = herdr_client::protocol::AgentStatus::Working;
        assert!(access.matches_agent(&client, &resumed).unwrap());
        resumed.agent_session.as_mut().unwrap().value = "replacement".into();
        assert!(!access.matches_agent(&client, &resumed).unwrap());
        resumed = agent.clone();
        resumed.pane_id.0 = "another-pane".into();
        assert!(!access.matches_agent(&client, &resumed).unwrap());
        resumed = agent;
        resumed.workspace_id.0 = "another-workspace".into();
        assert!(!access.matches_agent(&client, &resumed).unwrap());
    }

    #[test]
    fn process_identity_adopts_the_first_native_session_and_rejects_replacement() {
        let identity = AgentIdentity::ForegroundProcessGroup {
            group: 42,
            native_session: Arc::new(OnceLock::new()),
        };
        let resumed: AgentSession = serde_json::from_value(serde_json::json!({
            "source": "herdr:codex", "agent": "codex", "kind": "id", "value": "resumed"
        }))
        .unwrap();
        let mut replacement = resumed.clone();
        replacement.value = "replacement".into();
        assert!(identity.matches_session(None));
        assert!(identity.matches_session(Some(&resumed)));
        assert!(identity.clone().matches_session(Some(&resumed)));
        assert!(!identity.matches_session(Some(&replacement)));
        assert!(!identity.matches_session(None));
    }
}
