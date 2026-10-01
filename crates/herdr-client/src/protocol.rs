//! Herdr protocol boundaries used by the application and control processes.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::Result;

/// Herdr socket methods used by the reviewer.
pub mod method {
    /// Read the current Herdr session.
    pub const SESSION_SNAPSHOT: &str = "session.snapshot";
    /// List live agents.
    pub const AGENT_LIST: &str = "agent.list";
    /// Resolve one live agent.
    pub const AGENT_GET: &str = "agent.get";
    /// Read the visible contents of one live agent.
    pub const AGENT_READ: &str = "agent.read";
    /// Focus one live agent.
    pub const AGENT_FOCUS: &str = "agent.focus";
    /// Submit one prompt to a live agent.
    pub const AGENT_PROMPT: &str = "agent.prompt";
    /// Resolve one live pane.
    pub const PANE_GET: &str = "pane.get";
    /// Read the layout of the tab holding one pane.
    pub const PANE_LAYOUT: &str = "pane.layout";
    /// Open one plugin-owned pane.
    pub const PLUGIN_PANE_OPEN: &str = "plugin.pane.open";
    /// Focus one plugin-owned pane.
    pub const PLUGIN_PANE_FOCUS: &str = "plugin.pane.focus";
    /// Close one plugin-owned pane.
    pub const PLUGIN_PANE_CLOSE: &str = "plugin.pane.close";
}

/// A Herdr terminal pane ID.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PaneId(pub String);

/// A Herdr tab ID.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TabId(pub String);

/// A Herdr workspace ID.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct WorkspaceId(pub String);

/// A manifest pane entrypoint ID.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EntrypointId(pub String);

/// A pane placement that Herdr accepts when it opens a plugin pane.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PanePlacement {
    /// Open the pane as a split.
    Split,
}

/// The side of its target pane where a split pane opens.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    /// Beside the target, halving its width.
    Right,
    /// Below the target, halving its height.
    Down,
}

impl SplitDirection {
    /// The name Herdr's API and command line use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Down => "down",
        }
    }
}

/// A pane's size in terminal cells.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct PaneSize {
    /// Columns.
    pub width: u16,
    /// Rows.
    pub height: u16,
}

impl PaneSize {
    /// Split across the side that looks longer on screen, so both halves
    /// keep a usable shape. A cell is a little over twice as tall as it is
    /// wide, and code reads better wide than tall, so a pane splits beside
    /// only when it is at least two and a half times as wide as it is tall.
    pub fn split_direction(self) -> SplitDirection {
        if 2 * u32::from(self.width) >= 5 * u32::from(self.height) {
            SplitDirection::Right
        } else {
            SplitDirection::Down
        }
    }
}

/// The agent host that review delivery talks to: it resolves the target agent,
/// reports agent and process identity, and submits prompts.
///
/// [`crate::client::HerdrClient`] is the production adapter. The `memory` feature
/// adds an in-memory adapter for tests that need no Herdr server.
pub trait AgentPort: Send + Sync {
    /// Get a session snapshot, used to seed target resolution from the current focus.
    fn session_snapshot(&self) -> Result<SessionSnapshot>;

    /// List live agents.
    fn list_agents(&self) -> Result<Vec<Agent>>;

    /// Resolve a live agent by pane ID.
    fn get_agent(&self, pane_id: &PaneId) -> Result<Option<Agent>>;

    /// Inspect the processes currently owning a pane, without reading their environment.
    fn pane_process_info(&self, pane_id: &PaneId) -> Result<PaneProcessInfo>;

    /// Submit one complete prompt through Herdr's agent-aware boundary.
    fn prompt_agent(&self, pane_id: &PaneId, text: &str) -> Result<()>;
}

/// Pane read operations needed by the reviewer.
pub trait HerdrReader: Send + Sync {
    /// Read the visible text in an agent pane.
    fn read_agent_screen(&self, pane_id: &PaneId) -> Result<String>;

    /// List plugin-owned panes in one workspace.
    fn list_plugin_panes(&self, workspace_id: &WorkspaceId) -> Result<Vec<PluginPane>>;

    /// Measure one pane.
    fn pane_size(&self, pane_id: &PaneId) -> Result<PaneSize>;
}

/// Write operations needed by the reviewer.
pub trait HerdrWriter: Send + Sync {
    /// Open a review pane.
    fn open_plugin_pane(&self, request: &OpenPluginPane) -> Result<PluginPane>;

    /// Focus a plugin-owned pane.
    fn focus_plugin_pane(&self, pane_id: &PaneId) -> Result<()>;

    /// Focus an agent pane.
    fn focus_agent(&self, pane_id: &PaneId) -> Result<()>;

    /// Close a plugin-owned pane.
    fn close_plugin_pane(&self, pane_id: &PaneId) -> Result<()>;
}

/// The immutable action context supplied by Herdr.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PluginContext {
    /// The workspace in which the action started.
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    /// The tab in which the action started.
    #[serde(default)]
    pub tab_id: Option<TabId>,
    /// The focused pane when the action started.
    #[serde(default)]
    pub focused_pane_id: Option<PaneId>,
    /// The focused pane directory when the action started.
    #[serde(default)]
    pub focused_pane_cwd: Option<PathBuf>,
}

/// The session fields needed for initial target selection.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct SessionSnapshot {
    /// The focused workspace, if Herdr reports one.
    #[serde(default)]
    pub focused_workspace_id: Option<WorkspaceId>,
    /// The focused pane, if Herdr reports one.
    #[serde(default)]
    pub focused_pane_id: Option<PaneId>,
}

/// A live Herdr agent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Agent {
    /// The terminal pane that owns the agent.
    pub pane_id: PaneId,
    /// The tab that owns the pane.
    pub tab_id: TabId,
    /// The workspace that owns the pane.
    pub workspace_id: WorkspaceId,
    /// The optional user-facing agent name.
    #[serde(default)]
    pub name: Option<String>,
    /// The optional agent implementation name.
    #[serde(default)]
    pub display_agent: Option<String>,
    /// The canonical agent implementation name.
    #[serde(default)]
    pub agent: Option<String>,
    /// The current lifecycle state.
    pub agent_status: AgentStatus,
    /// The native session identity, when Herdr reports it.
    #[serde(default)]
    pub agent_session: Option<AgentSession>,
    /// The agent working directory.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

/// A Herdr agent lifecycle state.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    #[default]
    Unknown,
}

/// A native agent session identity reported by Herdr.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentSession {
    pub source: String,
    pub agent: String,
    pub kind: String,
    pub value: String,
}

/// Foreground process identities reported for a terminal pane.
#[derive(Clone, Debug, Deserialize)]
pub struct PaneProcessInfo {
    pub pane_id: PaneId,
    /// The process group currently attached to the pane's terminal.
    #[serde(default)]
    pub foreground_process_group_id: Option<u32>,
    #[serde(default)]
    pub foreground_processes: Vec<PaneProcess>,
}

/// A process in a pane's foreground process group.
#[derive(Clone, Debug, Deserialize)]
pub struct PaneProcess {
    pub pid: u32,
    pub name: String,
    pub argv: Option<Vec<String>>,
}

/// One typed event from Herdr that is relevant to the reviewer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HerdrEvent {
    /// The user focused a pane.
    PaneFocused(PaneId),
    /// Herdr detected or released an agent process in a pane.
    AgentDetected {
        pane_id: PaneId,
        workspace_id: WorkspaceId,
        agent: Option<String>,
        released: bool,
        final_status: Option<AgentStatus>,
    },
}

/// A pane owned by this plugin.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PluginPane {
    /// The Herdr pane ID.
    pub pane_id: PaneId,
    /// The owning tab ID.
    pub tab_id: TabId,
    /// The owning workspace ID.
    pub workspace_id: WorkspaceId,
    /// The manifest pane entrypoint.
    pub entrypoint_id: EntrypointId,
}

/// Parameters for `plugin.pane.open`.
#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct OpenPluginPane {
    /// The manifest pane entrypoint.
    pub entrypoint: EntrypointId,
    /// The required split placement.
    pub placement: PanePlacement,
    /// The pane beside which Herdr opens the review pane.
    pub target_pane_id: PaneId,
    /// The side of the target where the pane opens; `None` lets Herdr choose.
    pub direction: Option<SplitDirection>,
    /// The jj working directory.
    pub cwd: PathBuf,
    /// Whether Herdr focuses the new pane.
    pub focus: bool,
}

/// The shared last-focused agent target for one Herdr workspace.
#[derive(Clone, Debug)]
pub struct AgentTarget {
    workspace_id: WorkspaceId,
    focused_pane_ids: Arc<Mutex<Vec<PaneId>>>,
}

impl AgentTarget {
    /// Create a target from the pane that opened the reviewer.
    pub fn new(workspace_id: WorkspaceId, focused_pane_id: Option<PaneId>) -> Self {
        Self {
            workspace_id,
            focused_pane_ids: Arc::new(Mutex::new(focused_pane_id.into_iter().collect())),
        }
    }

    /// Seed the target from the current session focus.
    fn initialize(&mut self, port: &(impl AgentPort + ?Sized)) -> Result<()> {
        let snapshot = port.session_snapshot()?;
        if snapshot.focused_workspace_id.as_ref() == Some(&self.workspace_id)
            && let Some(pane_id) = snapshot.focused_pane_id
        {
            self.observe_focus(&pane_id);
        }
        Ok(())
    }

    /// Record a pane focus for validation before selecting the active agent.
    ///
    /// # Panics
    /// Panics if another thread poisoned the shared focus lock.
    pub fn observe_focus(&mut self, pane_id: &PaneId) {
        let mut focused = self.focused_pane_ids.lock().expect("agent focus lock");
        focused.retain(|previous| previous != pane_id);
        focused.push(pane_id.clone());
    }

    /// Resolve the current same-workspace implementation agent.
    pub fn resolve(&mut self, port: &(impl AgentPort + ?Sized)) -> Result<Option<Agent>> {
        let agents = port.list_agents()?;
        self.initialize(port)?;
        if let Some(agent) = self.current_agent(&agents) {
            return Ok(Some(agent));
        }

        let mut same_workspace_agents = agents
            .into_iter()
            .filter(|agent| agent.workspace_id == self.workspace_id);
        let only_agent = same_workspace_agents.next();
        if same_workspace_agents.next().is_some() {
            return Ok(None);
        }
        Ok(only_agent)
    }

    fn current_agent(&self, agents: &[Agent]) -> Option<Agent> {
        self.focused_pane_ids
            .lock()
            .expect("agent focus lock")
            .iter()
            .rev()
            .find_map(|pane_id| {
                agents
                    .iter()
                    .find(|agent| {
                        agent.pane_id == *pane_id && agent.workspace_id == self.workspace_id
                    })
                    .cloned()
            })
    }
}

#[cfg(test)]
#[path = "protocol.tests.rs"]
mod tests;
