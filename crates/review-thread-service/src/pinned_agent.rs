use std::sync::{Arc, Mutex};

use herdr_client::{
    client::HerdrClient,
    protocol::{Agent, HerdrReader},
};

/// A conversation identity shared by queued delivery and subsequent MCP access checks.
#[derive(Clone, Debug)]
pub struct PinnedAgent(Arc<Mutex<Pin>>);

#[derive(Debug)]
struct Pin {
    agent: Agent,
    /// A Retry without a native session may target only this foreground process.
    process_group: Option<u32>,
    follow_selected_until_attempt: bool,
}

impl Pin {
    fn verify_process(&self, client: &HerdrClient, agent: &Agent) -> Result<(), String> {
        let Some(group) = self.process_group else {
            return Ok(());
        };
        let current = client
            .pane_process_info(&agent.pane_id)
            .map_err(|error| error.to_string())?
            .foreground_process_group_id;
        if current != Some(group) {
            return Err("The selected agent process changed; retry the interrupted turn".into());
        }
        Ok(())
    }

    fn session_ready(&self, agent: &Agent) -> Result<bool, String> {
        if self.follow_selected_until_attempt {
            return Ok(true);
        }
        let Some(known) = &self.agent.agent_session else {
            return Ok(true);
        };
        let Some(current) = &agent.agent_session else {
            return Ok(false);
        };
        if current.agent != known.agent
            || current.kind != known.kind
            || current.value != known.value
        {
            return Err(
                "The selected pane is now running a different agent conversation; start a new pass"
                    .into(),
            );
        }
        Ok(true)
    }
}

impl PinnedAgent {
    pub fn new(agent: Agent) -> Self {
        Self(Arc::new(Mutex::new(Pin {
            agent,
            process_group: None,
            follow_selected_until_attempt: false,
        })))
    }

    /// Follow the chosen pane until the queued prompt actually begins delivery.
    pub fn for_selected_prompt(agent: Agent) -> Self {
        Self(Arc::new(Mutex::new(Pin {
            agent,
            process_group: None,
            follow_selected_until_attempt: true,
        })))
    }

    pub fn for_retry(agent: Agent, client: &HerdrClient) -> Result<Self, String> {
        let process_group = if agent.agent_session.is_none() {
            Some(
                client
                    .pane_process_info(&agent.pane_id)
                    .map_err(|error| error.to_string())?
                    .foreground_process_group_id
                    .filter(|group| *group != 0)
                    .ok_or("Waiting for the selected agent process identity")?,
            )
        } else {
            None
        };
        Ok(Self(Arc::new(Mutex::new(Pin {
            agent,
            process_group,
            follow_selected_until_attempt: true,
        }))))
    }

    /// The attempt has selected its recipient; later MCP access uses that identity.
    pub fn seal_attempt(&self) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "The selected agent identity is unavailable")?
            .follow_selected_until_attempt = false;
        Ok(())
    }

    pub fn known_agent(&self) -> Option<Agent> {
        self.0.lock().ok().map(|pin| pin.agent.clone())
    }

    /// Missing native identity is transient; a known replacement is an error.
    pub fn current(&self, client: &HerdrClient) -> Result<Option<Agent>, String> {
        let mut previous = self
            .0
            .lock()
            .map_err(|_| "The selected agent identity is unavailable")?;
        let current = client
            .get_agent(&previous.agent.pane_id)
            .map_err(|error| error.to_string())?
            .ok_or("The selected agent is no longer available")?;
        if current.pane_id != previous.agent.pane_id
            || current.workspace_id != previous.agent.workspace_id
            || current.agent != previous.agent.agent
        {
            return Err(
                "The selected pane is now running a different agent; start a new pass".into(),
            );
        }
        previous.verify_process(client, &current)?;
        if !previous.session_ready(&current)? {
            return Ok(None);
        }
        previous.agent = current.clone();
        if current.agent_session.is_some() {
            previous.process_group = None;
        }
        Ok(Some(current))
    }
}
