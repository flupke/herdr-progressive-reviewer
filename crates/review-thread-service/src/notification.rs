use herdr_client::client::HerdrClient;
use herdr_client::protocol::{Agent, HerdrReader};
use review_threads::ThreadId;

/// A notification waiting for Herdr to identify the selected native session.
pub(super) struct Notification {
    pub(super) agent: Agent,
    pub(super) retries: Vec<ThreadId>,
}

impl Notification {
    pub(super) fn new(agent: Agent) -> Self {
        Self {
            agent,
            retries: Vec::new(),
        }
    }

    pub(super) fn current_agent(&self, client: &HerdrClient) -> Result<Option<Agent>, String> {
        let Some(current) = client
            .get_agent(&self.agent.pane_id)
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        if current.workspace_id != self.agent.workspace_id || current.agent != self.agent.agent {
            return Err(
                "The selected pane has changed agents; post a follow-up in the reviewer".into(),
            );
        }
        if current.agent_session.is_none() {
            return Ok(None);
        }
        if self.agent.agent_session.is_some() && current.agent_session != self.agent.agent_session {
            return Err(
                "The selected pane has changed agent sessions; post a follow-up in the reviewer"
                    .into(),
            );
        }
        Ok(Some(current))
    }
}
