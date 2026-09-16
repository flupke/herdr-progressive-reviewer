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
    /// Open one plugin-owned pane.
    pub const PLUGIN_PANE_OPEN: &str = "plugin.pane.open";
    /// Focus one plugin-owned pane.
    pub const PLUGIN_PANE_FOCUS: &str = "plugin.pane.focus";
    /// Close one plugin-owned pane.
    pub const PLUGIN_PANE_CLOSE: &str = "plugin.pane.close";
    /// Insert literal text into a terminal pane.
    pub const PANE_SEND_TEXT: &str = "pane.send_text";
    /// Send key presses to a terminal pane.
    pub const PANE_SEND_KEYS: &str = "pane.send_keys";
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

/// Read operations needed by the reviewer.
pub trait HerdrReader: Send + Sync {
    /// Get a session snapshot.
    fn session_snapshot(&self) -> Result<SessionSnapshot>;

    /// List live agents.
    fn list_agents(&self) -> Result<Vec<Agent>>;

    /// Resolve a live agent by pane ID.
    fn get_agent(&self, pane_id: &PaneId) -> Result<Option<Agent>>;

    /// Read the visible text in an agent pane.
    fn read_agent_screen(&self, pane_id: &PaneId) -> Result<String>;

    /// List plugin-owned panes in one workspace.
    fn list_plugin_panes(&self, workspace_id: &WorkspaceId) -> Result<Vec<PluginPane>>;
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

    /// Insert text without a submit key.
    fn send_text(&self, pane_id: &PaneId, text: &str) -> Result<()>;

    /// Send key presses to a pane.
    fn send_keys(&self, pane_id: &PaneId, keys: &[&str]) -> Result<()>;
}

/// Agent-aware prompt submission needed by review-guide generation.
pub trait AgentPrompter: Send + Sync {
    /// Submit one complete prompt through Herdr's agent-aware boundary.
    fn prompt_agent(&self, pane_id: &PaneId, text: &str) -> Result<()>;
}

/// The immutable action context supplied by Herdr.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
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
    /// The lifecycle status of an agent changed.
    AgentStatusChanged {
        pane_id: PaneId,
        workspace_id: WorkspaceId,
        agent: Option<String>,
        status: AgentStatus,
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
    /// The jj working directory.
    pub cwd: PathBuf,
    /// Whether Herdr focuses the new pane.
    pub focus: bool,
}

/// The result of inserting an excerpt into an agent pane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InsertResult {
    /// Text was inserted into the named agent.
    Inserted { agent_name: String },
    /// No live same-workspace agent is available.
    NoAgent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentInputMode {
    VimNormal,
    Other,
}

impl AgentInputMode {
    fn detect(screen: &str) -> Self {
        if screen.contains("Vim: Normal") {
            Self::VimNormal
        } else {
            Self::Other
        }
    }
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
    fn initialize(&mut self, reader: &impl HerdrReader) -> Result<()> {
        let snapshot = reader.session_snapshot()?;
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
    pub fn resolve(&mut self, reader: &impl HerdrReader) -> Result<Option<Agent>> {
        let agents = reader.list_agents()?;
        self.initialize(reader)?;
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

    /// Resolve the target again and insert text without submission.
    pub fn insert<C>(&mut self, client: &C, text: &str) -> Result<InsertResult>
    where
        C: HerdrReader + HerdrWriter,
    {
        let Some(agent) = self.resolve(client)? else {
            return Ok(InsertResult::NoAgent);
        };

        let screen = client.read_agent_screen(&agent.pane_id)?;
        if AgentInputMode::detect(&screen) == AgentInputMode::VimNormal {
            client.send_keys(&agent.pane_id, &["i"])?;
        }
        client.send_text(&agent.pane_id, &format!("{text}\n\n"))?;
        client.focus_agent(&agent.pane_id)?;
        Ok(InsertResult::Inserted {
            agent_name: agent
                .name
                .or(agent.display_agent)
                .unwrap_or(agent.pane_id.0),
        })
    }
}

#[cfg(test)]
#[path = "protocol.tests.rs"]
mod tests;
