use std::sync::{Arc, Mutex};

use herdr_client::protocol::{Agent, AgentPort};

use crate::agent_identity::{AgentIdentity, Change, IdentityRules, Verdict};

const PROCESS_CHANGED: &str = "The selected agent process changed; retry the interrupted turn";

/// A conversation identity shared by queued delivery and subsequent MCP access checks.
#[derive(Clone, Debug)]
pub struct PinnedAgent(Arc<Mutex<Pin>>);

#[derive(Debug)]
struct Pin {
    /// The latest live agent that matched the identity.
    agent: Agent,
    identity: AgentIdentity,
}

impl PinnedAgent {
    fn pin(agent: Agent, identity: AgentIdentity) -> Self {
        Self(Arc::new(Mutex::new(Pin { agent, identity })))
    }

    pub fn new(agent: Agent) -> Self {
        let identity = AgentIdentity::new(&agent, IdentityRules::PINNED_AGENT);
        Self::pin(agent, identity)
    }

    /// Follow the chosen pane until the queued prompt actually begins delivery.
    pub fn for_selected_prompt(agent: Agent) -> Self {
        let identity = AgentIdentity::new(&agent, IdentityRules::PINNED_AGENT).following();
        Self::pin(agent, identity)
    }

    /// A Retry without a native session may target only the current foreground process.
    pub fn for_retry(agent: Agent, port: &dyn AgentPort) -> Result<Self, String> {
        let identity = AgentIdentity::new(&agent, IdentityRules::PINNED_AGENT)
            .following()
            .bind_process_without_session(port)
            .map_err(|error| {
                error.into_message("Waiting for the selected agent process identity")
            })?;
        Ok(Self::pin(agent, identity))
    }

    /// The attempt has selected its recipient; later MCP access uses that identity.
    pub fn seal_attempt(&self) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "The selected agent identity is unavailable")?
            .identity
            .seal();
        Ok(())
    }

    pub fn known_agent(&self) -> Option<Agent> {
        self.0.lock().ok().map(|pin| pin.agent.clone())
    }

    /// Missing native identity is transient; a known replacement is an error.
    pub fn current(&self, port: &dyn AgentPort) -> Result<Option<Agent>, String> {
        let mut pin = self
            .0
            .lock()
            .map_err(|_| "The selected agent identity is unavailable")?;
        let current = port
            .get_agent(&pin.agent.pane_id)
            .map_err(|error| error.to_string())?
            .ok_or("The selected agent is no longer available")?;
        let verdict = pin
            .identity
            .check(port, &current)
            .map_err(|error| error.into_message(PROCESS_CHANGED))?;
        match verdict {
            Verdict::Same => {
                pin.agent = current.clone();
                Ok(Some(current))
            }
            Verdict::SessionMissing => Ok(None),
            Verdict::Changed(Change::Agent) => {
                Err("The selected pane is now running a different agent; Reset to start a new round".into())
            }
            Verdict::Changed(Change::ProcessGroup) => Err(PROCESS_CHANGED.into()),
            Verdict::Changed(Change::Session) => Err(
                "The selected pane is now running a different agent conversation; Reset to start a new round"
                    .into(),
            ),
        }
    }
}

#[cfg(test)]
#[path = "pinned_agent.tests.rs"]
mod tests;
