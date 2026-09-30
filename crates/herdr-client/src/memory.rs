//! An in-memory agent host for tests that exercise agent delivery without Herdr.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::Result;
use crate::protocol::{Agent, AgentPort, PaneId, PaneProcessInfo, SessionSnapshot};

/// A prompt submitted to one agent pane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SentPrompt {
    pub pane_id: PaneId,
    pub text: String,
}

/// Live agents, pane process groups and submitted prompts kept in memory.
///
/// Clones share the same host, so a test can keep one handle while the code under
/// test owns another.
#[derive(Clone, Debug, Default)]
pub struct InMemoryAgents(Arc<Mutex<Host>>);

#[derive(Debug, Default)]
struct Host {
    session: SessionSnapshot,
    agents: Vec<Agent>,
    process_groups: HashMap<PaneId, u32>,
    prompts: Vec<SentPrompt>,
}

impl InMemoryAgents {
    fn host(&self) -> MutexGuard<'_, Host> {
        self.0.lock().expect("in-memory agent host lock")
    }

    /// Replace the session snapshot that target resolution reads.
    pub fn set_session(&self, session: SessionSnapshot) {
        self.host().session = session;
    }

    /// Add a live agent, or replace the agent already running in its pane.
    pub fn upsert_agent(&self, agent: Agent) {
        let mut host = self.host();
        match host
            .agents
            .iter_mut()
            .find(|existing| existing.pane_id == agent.pane_id)
        {
            Some(existing) => *existing = agent,
            None => host.agents.push(agent),
        }
    }

    /// Remove the agent running in `pane_id`.
    pub fn remove_agent(&self, pane_id: &PaneId) {
        self.host().agents.retain(|agent| agent.pane_id != *pane_id);
    }

    /// Set the foreground process group Herdr reports for `pane_id`.
    pub fn set_process_group(&self, pane_id: &PaneId, group: u32) {
        self.host().process_groups.insert(pane_id.clone(), group);
    }

    /// Every prompt submitted so far, in submission order.
    pub fn prompts(&self) -> Vec<SentPrompt> {
        self.host().prompts.clone()
    }
}

impl AgentPort for InMemoryAgents {
    fn session_snapshot(&self) -> Result<SessionSnapshot> {
        Ok(self.host().session.clone())
    }

    fn list_agents(&self) -> Result<Vec<Agent>> {
        Ok(self.host().agents.clone())
    }

    fn get_agent(&self, pane_id: &PaneId) -> Result<Option<Agent>> {
        Ok(self
            .host()
            .agents
            .iter()
            .find(|agent| agent.pane_id == *pane_id)
            .cloned())
    }

    fn pane_process_info(&self, pane_id: &PaneId) -> Result<PaneProcessInfo> {
        Ok(PaneProcessInfo {
            pane_id: pane_id.clone(),
            foreground_process_group_id: self.host().process_groups.get(pane_id).copied(),
            foreground_processes: Vec::new(),
        })
    }

    fn prompt_agent(&self, pane_id: &PaneId, text: &str) -> Result<()> {
        self.host().prompts.push(SentPrompt {
            pane_id: pane_id.clone(),
            text: text.to_owned(),
        });
        Ok(())
    }
}
