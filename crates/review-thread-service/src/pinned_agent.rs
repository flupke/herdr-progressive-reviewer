use std::sync::{Arc, Mutex};

use herdr_client::{
    client::HerdrClient,
    protocol::{Agent, HerdrReader},
};

/// A conversation identity shared by queued delivery and subsequent MCP access checks.
#[derive(Clone, Debug)]
pub struct PinnedAgent(Arc<Mutex<Agent>>);

impl PinnedAgent {
    pub fn new(agent: Agent) -> Self {
        Self(Arc::new(Mutex::new(agent)))
    }

    pub fn known_agent(&self) -> Option<Agent> {
        self.0.lock().ok().map(|agent| agent.clone())
    }

    /// Missing native identity is transient; a known replacement is an error.
    pub fn current(&self, client: &HerdrClient) -> Result<Option<Agent>, String> {
        let mut previous = self
            .0
            .lock()
            .map_err(|_| "The selected agent identity is unavailable")?;
        let current = client
            .get_agent(&previous.pane_id)
            .map_err(|error| error.to_string())?
            .ok_or("The selected agent is no longer available")?;
        if current.pane_id != previous.pane_id
            || current.workspace_id != previous.workspace_id
            || current.agent != previous.agent
        {
            return Err(
                "The selected pane is now running a different agent; start a new pass".into(),
            );
        }
        if let Some(known) = &previous.agent_session {
            let Some(session) = &current.agent_session else {
                return Ok(None);
            };
            if session.agent != known.agent
                || session.kind != known.kind
                || session.value != known.value
            {
                return Err("The selected pane is now running a different agent conversation; start a new pass".into());
            }
        }
        // Adopt first native discovery without mistaking reporter metadata for identity.
        *previous = current.clone();
        Ok(Some(current))
    }
}
