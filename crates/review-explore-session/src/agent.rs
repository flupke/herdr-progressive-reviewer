//! The agent that receives this session's prompts.

use herdr_client::protocol::Agent;
use review_thread_service::PinnedAgent;

use crate::ExploreSession;

impl ExploreSession {
    pub(crate) fn active_agent(&mut self) -> eyre::Result<PinnedAgent> {
        if let Some(agent) = &self.state.agent {
            agent.current(&*self.agents).map_err(eyre::Report::msg)?;
            return Ok(agent.clone());
        }
        self.select_agent()
    }

    pub(crate) fn select_agent(&mut self) -> eyre::Result<PinnedAgent> {
        self.state.agent = None;
        let selected = self
            .target
            .resolve(&*self.agents)?
            .ok_or_else(|| eyre::eyre!("Selected implementation agent is unavailable"))?;
        let agent = PinnedAgent::for_selected_prompt(selected);
        self.state.agent = Some(agent.clone());
        Ok(agent)
    }

    pub(crate) fn retry_agent(&mut self) -> eyre::Result<Agent> {
        self.target
            .resolve(&*self.agents)?
            .ok_or_else(|| eyre::eyre!("Selected implementation agent is unavailable"))
    }
}
