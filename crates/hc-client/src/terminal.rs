//! herdr-core's terminals, as its API gives them, and the agents and processes the reviewer
//! reads from them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use herdr_client::protocol::{
    Agent, AgentStatus, PaneId, PaneProcess, PaneProcessInfo, TabId, WorkspaceId,
};
use serde::Deserialize;

/// The label herdr-app gives the terminals of a space: its ID.
const SPACE_LABEL: &str = "herdr.space";
/// The label herdr-app gives the terminals of a tab: its ID.
const TAB_LABEL: &str = "herdr.tab";

/// One terminal of herdr-core, live or finished (`Terminal` in `hc schema`).
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Terminal {
    /// `t7`: the terminal, for good, on its core.
    pub(crate) address: String,
    #[serde(default)]
    pub(crate) labels: BTreeMap<String, String>,
    #[serde(default)]
    name: Option<String>,
    /// The folder it started in.
    #[serde(default)]
    cwd: Option<PathBuf>,
    /// What its program last reported with OSC 7501; `None` when it never did, or once it
    /// exited.
    #[serde(default)]
    pub(crate) status: Option<Status>,
    /// The leader of its foreground process group; `None` once it finished.
    #[serde(default)]
    foreground: Option<Foreground>,
    /// `live` or `finished`.
    #[serde(default)]
    state: String,
}

/// A program's OSC 7501 report: its own words, which herdr-core does not translate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct Status {
    /// `idle`, `working`, `done`, `blocked` or `error`.
    state: String,
    /// The program that reports, such as `claude-code` or `codex`.
    #[serde(default)]
    app: Option<String>,
}

/// The leader of a terminal's foreground process group.
#[derive(Clone, Debug, Deserialize)]
struct Foreground {
    pid: u32,
    name: String,
    argv: Vec<String>,
    /// Its working directory now.
    #[serde(default)]
    cwd: Option<PathBuf>,
}

/// The space and the tab of herdr-app that a terminal with `labels` shows in.
pub(crate) fn place_of(labels: &BTreeMap<String, String>) -> Option<(WorkspaceId, TabId)> {
    let space = labels.get(SPACE_LABEL).cloned().map(WorkspaceId)?;
    let tab = labels.get(TAB_LABEL).cloned().map(TabId)?;
    Some((space, tab))
}

impl Status {
    /// The status in Herdr's words. `error`, which Herdr lacks, is unknown: the agent neither
    /// waits for a prompt nor works on one.
    pub(crate) fn agent_status(&self) -> AgentStatus {
        match self.state.as_str() {
            "idle" => AgentStatus::Idle,
            "working" => AgentStatus::Working,
            "blocked" => AgentStatus::Blocked,
            "done" => AgentStatus::Done,
            _ => AgentStatus::Unknown,
        }
    }

    /// The agent's name as Herdr gives it, which the reviewer matches to pick an agent's forks
    /// and hooks: herdr-core reports Claude Code as `claude-code`, Herdr as `claude`.
    pub(crate) fn agent(&self) -> Option<String> {
        self.app.as_deref().map(|app| match app {
            "claude-code" => "claude".to_owned(),
            other => other.to_owned(),
        })
    }
}

impl Terminal {
    fn pane_id(&self) -> PaneId {
        PaneId(self.address.clone())
    }

    /// The space and the tab of herdr-app the terminal shows in, by its labels.
    pub(crate) fn place(&self) -> Option<(WorkspaceId, TabId)> {
        place_of(&self.labels)
    }

    /// Whether its program still runs: not once it finished.
    pub(crate) fn is_live(&self) -> bool {
        self.state == "live"
    }

    /// The agent the terminal runs: a program that reports its status, in a tab of herdr-app.
    /// herdr-core keeps no agent session, so the agent has none.
    pub(crate) fn agent(&self) -> Option<Agent> {
        let status = self.status.as_ref()?;
        let (workspace_id, tab_id) = self.place()?;
        Some(Agent {
            pane_id: self.pane_id(),
            tab_id,
            workspace_id,
            name: self.name.clone(),
            display_agent: status.app.clone(),
            agent: status.agent(),
            agent_status: status.agent_status(),
            agent_session: None,
            cwd: self
                .foreground
                .as_ref()
                .and_then(|foreground| foreground.cwd.clone())
                .or_else(|| self.cwd.clone()),
        })
    }

    /// The terminal's foreground process group, which herdr-core names by its leader alone.
    pub(crate) fn process_info(&self) -> PaneProcessInfo {
        PaneProcessInfo {
            pane_id: self.pane_id(),
            foreground_process_group_id: self.foreground.as_ref().map(|leader| leader.pid),
            foreground_processes: self
                .foreground
                .iter()
                .map(|leader| PaneProcess {
                    pid: leader.pid,
                    name: leader.name.clone(),
                    argv: Some(leader.argv.clone()),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
#[path = "terminal.tests.rs"]
mod tests;
