//! Herdr protocol boundaries used by the application and control processes.

use std::path::PathBuf;

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
    /// Open one plugin-owned pane.
    pub const PLUGIN_PANE_OPEN: &str = "plugin.pane.open";
    /// Focus one plugin-owned pane.
    pub const PLUGIN_PANE_FOCUS: &str = "plugin.pane.focus";
    /// Close one plugin-owned pane.
    pub const PLUGIN_PANE_CLOSE: &str = "plugin.pane.close";
    /// Insert literal text into a terminal pane.
    pub const PANE_SEND_TEXT: &str = "pane.send_text";
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

    /// List plugin-owned panes in one workspace.
    fn list_plugin_panes(&self, workspace_id: &WorkspaceId) -> Result<Vec<PluginPane>>;
}

/// Write operations needed by the reviewer.
pub trait HerdrWriter: Send + Sync {
    /// Open a review pane.
    fn open_plugin_pane(&self, request: &OpenPluginPane) -> Result<PluginPane>;

    /// Focus a plugin-owned pane.
    fn focus_plugin_pane(&self, pane_id: &PaneId) -> Result<()>;

    /// Close a plugin-owned pane.
    fn close_plugin_pane(&self, pane_id: &PaneId) -> Result<()>;

    /// Insert text without a submit key.
    fn send_text(&self, pane_id: &PaneId, text: &str) -> Result<()>;
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
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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
    /// The agent working directory.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

/// A pane owned by this plugin.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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
