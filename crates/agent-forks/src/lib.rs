//! Run-ahead's host for whichever agent runs in the pane: [`AgentForks`] hands each call to the
//! Claude Code host or to the Codex host, by the agent Herdr reports, or by the fields the host
//! keeps of a fork. The Explore session sees one [`ForkHost`] and never knows which agent it
//! drives.

use herdr_client::protocol::{Agent, PaneId};
use review_run_ahead::{
    ForkEnd, ForkHost, ForkPoint, ForkStart, ForkTrace, HostFields, OwnedFork, PaneWatch,
    StatusReport, SwitchFailure, SwitchTo,
};

/// The run-ahead hosts of the agents run-ahead forks.
pub struct AgentForks {
    claude: claude_fork::ClaudeForks,
    codex: codex_fork::CodexForks,
}

impl AgentForks {
    pub fn new(claude: claude_fork::ClaudeForks, codex: codex_fork::CodexForks) -> Self {
        Self { claude, codex }
    }

    /// The host of `agent`, when run-ahead forks it.
    fn of_agent(&self, agent: &Agent) -> Option<&dyn ForkHost> {
        match agent.agent.as_deref() {
            Some("claude") => Some(&self.claude),
            Some("codex") => Some(&self.codex),
            _ => None,
        }
    }

    /// The host that keeps `fields`: a record saved before Codex forks names Claude Code's.
    fn of_fields(&self, fields: &HostFields) -> &dyn ForkHost {
        if codex_fork::owns(fields) {
            &self.codex
        } else {
            &self.claude
        }
    }
}

/// Why run-ahead never forks `agent`, an agent neither host forks.
fn not_forked(agent: &Agent) -> String {
    format!(
        "run-ahead forks Claude Code and Codex, and the agent's pane runs {}",
        agent
            .agent
            .as_deref()
            .unwrap_or("an agent Herdr does not name")
    )
}

impl ForkHost for AgentForks {
    fn watch(&self, pane: &PaneId, report: StatusReport) -> PaneWatch {
        // Both hosts watch the agent's status through Herdr alike.
        self.claude.watch(pane, report)
    }

    fn session(&self, agent: &Agent) -> Option<String> {
        self.of_agent(agent)?.session(agent)
    }

    fn unhooked(&self, agent: &Agent) -> Option<String> {
        self.of_agent(agent)?.unhooked(agent)
    }

    fn can_move(&self, agent: &Agent) -> bool {
        self.of_agent(agent)
            .is_some_and(|host| host.can_move(agent))
    }

    fn unforkable(&self, agent: &Agent) -> Option<String> {
        match self.of_agent(agent) {
            Some(host) => host.unforkable(agent),
            None => Some(not_forked(agent)),
        }
    }

    fn point(&self, agent: &Agent) -> Result<ForkPoint, String> {
        self.of_agent(agent)
            .ok_or_else(|| not_forked(agent))?
            .point(agent)
    }

    fn last_entry(&self, point: &ForkPoint) -> Option<String> {
        self.of_fields(&point.host).last_entry(point)
    }

    fn create(&self, point: &ForkPoint) -> Result<OwnedFork, String> {
        self.of_fields(&point.host).create(point)
    }

    fn start(
        &self,
        start: ForkStart<'_>,
        prompt: String,
        ended: Box<dyn FnOnce(ForkEnd) + Send>,
    ) -> Result<HostFields, String> {
        self.of_fields(start.fork.host).start(start, prompt, ended)
    }

    fn discard(&self, fork: ForkTrace<'_>, done: Box<dyn FnOnce() + Send>) {
        self.of_fields(fork.host).discard(fork, done);
    }

    fn switch(
        &self,
        switch: SwitchTo<'_>,
        done: Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>,
    ) {
        self.of_fields(switch.fork.host).switch(switch, done);
    }

    fn resume(
        &self,
        pane: &PaneId,
        from: ForkTrace<'_>,
        session: &str,
        done: Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>,
    ) {
        self.of_fields(from.host).resume(pane, from, session, done);
    }

    fn log(&self, line: &str) {
        // Both hosts write run-ahead's one log.
        self.claude.log(line);
    }
}
