//! Herdr protocol boundaries used by the application and control processes.

use std::borrow::Cow;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::repository::Repository;

const REVIEW_ENTRYPOINT: &str = "review";

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

/// A manifest action for the review pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneAction {
    /// Open or focus the review pane.
    Open,
    /// Close the review pane if it exists.
    Close,
    /// Close an open pane or open a missing pane.
    Toggle,
}

/// The visible result of one pane action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaneActionResult {
    /// A new pane was opened.
    Opened(PaneId),
    /// An existing pane was focused.
    Focused(PaneId),
    /// One or more panes were closed.
    Closed,
    /// Close found no pane and changed nothing.
    AlreadyClosed,
}

/// Idempotent review-pane actions over a Herdr client.
#[derive(Debug)]
pub struct PaneActions<'a, C> {
    client: &'a C,
}

impl<'a, C> PaneActions<'a, C>
where
    C: HerdrReader + HerdrWriter,
{
    /// Bind pane actions to one Herdr client.
    pub fn new(client: &'a C) -> Self {
        Self { client }
    }

    /// Run one action using only the immutable action context.
    pub fn run(&self, action: PaneAction, context: &PluginContext) -> Result<PaneActionResult> {
        let workspace = context
            .workspace_id
            .as_ref()
            .ok_or_else(|| missing_context("workspace ID"))?;
        let panes = self.review_panes(workspace)?;
        match action {
            PaneAction::Open => self.open(context, &panes),
            PaneAction::Toggle if panes.is_empty() => self.open(context, &panes),
            PaneAction::Close | PaneAction::Toggle => self.close(panes),
        }
    }

    fn open(&self, context: &PluginContext, panes: &[PluginPane]) -> Result<PaneActionResult> {
        if let Some(pane) = panes.first() {
            self.close_duplicates(panes)?;
            self.client.focus_plugin_pane(&pane.pane_id)?;
            return Ok(PaneActionResult::Focused(pane.pane_id.clone()));
        }

        let target_pane_id = context
            .focused_pane_id
            .clone()
            .ok_or_else(|| missing_context("focused pane ID"))?;
        let cwd = context
            .focused_pane_cwd
            .clone()
            .ok_or_else(|| missing_context("focused pane directory"))?;
        Repository::discover(&cwd)?;
        let opened = self.client.open_plugin_pane(&OpenPluginPane {
            entrypoint: EntrypointId(REVIEW_ENTRYPOINT.to_owned()),
            placement: PanePlacement::Split,
            target_pane_id,
            cwd,
            focus: true,
        })?;

        let workspace = context
            .workspace_id
            .as_ref()
            .expect("run checked the workspace ID");
        let panes = self.review_panes(workspace)?;
        let winner = panes.first().unwrap_or(&opened).pane_id.clone();
        self.close_duplicates(&panes)?;
        if winner == opened.pane_id {
            Ok(PaneActionResult::Opened(winner))
        } else {
            self.client.focus_plugin_pane(&winner)?;
            Ok(PaneActionResult::Focused(winner))
        }
    }

    fn close(&self, panes: Vec<PluginPane>) -> Result<PaneActionResult> {
        if panes.is_empty() {
            return Ok(PaneActionResult::AlreadyClosed);
        }
        for pane in panes {
            self.client.close_plugin_pane(&pane.pane_id)?;
        }
        Ok(PaneActionResult::Closed)
    }

    fn review_panes(&self, workspace: &WorkspaceId) -> Result<Vec<PluginPane>> {
        Ok(self
            .client
            .list_plugin_panes(workspace)?
            .into_iter()
            .filter(|pane| pane.entrypoint_id.0 == REVIEW_ENTRYPOINT)
            .collect())
    }

    fn close_duplicates(&self, panes: &[PluginPane]) -> Result<()> {
        for pane in panes.iter().skip(1) {
            self.client.close_plugin_pane(&pane.pane_id)?;
        }
        Ok(())
    }
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

    fn prepare(self, text: &str) -> Cow<'_, str> {
        match self {
            Self::VimNormal => Cow::Owned(format!("i{text}")),
            Self::Other => Cow::Borrowed(text),
        }
    }
}

/// The last focused agent target for one Herdr workspace.
#[derive(Debug)]
pub struct AgentTarget {
    workspace_id: WorkspaceId,
    last_agent_pane_id: Option<PaneId>,
}

impl AgentTarget {
    /// Create an empty target for one workspace.
    pub fn new(workspace_id: WorkspaceId) -> Self {
        Self {
            workspace_id,
            last_agent_pane_id: None,
        }
    }

    /// Seed the target from the current session focus.
    pub fn initialize(&mut self, reader: &impl HerdrReader) -> Result<()> {
        let snapshot = reader.session_snapshot()?;
        if snapshot.focused_workspace_id.as_ref() == Some(&self.workspace_id)
            && let Some(pane_id) = snapshot.focused_pane_id
        {
            self.observe_focus(reader, &pane_id)?;
        }
        Ok(())
    }

    /// Record a focused pane if it is a same-workspace agent.
    pub fn observe_focus(&mut self, reader: &impl HerdrReader, pane_id: &PaneId) -> Result<()> {
        if let Some(agent) = reader.get_agent(pane_id)? {
            self.observe_agent_focus(&agent);
        }
        Ok(())
    }

    /// Record a focused agent event from this workspace.
    pub fn observe_agent_focus(&mut self, agent: &Agent) {
        if agent.workspace_id == self.workspace_id {
            self.last_agent_pane_id = Some(agent.pane_id.clone());
        }
    }

    /// Resolve the target again and insert text without submission.
    pub fn insert<C>(&mut self, client: &C, text: &str) -> Result<InsertResult>
    where
        C: HerdrReader + HerdrWriter,
    {
        let Some(pane_id) = self.last_agent_pane_id.as_ref() else {
            return Ok(InsertResult::NoAgent);
        };
        let Some(agent) = client.get_agent(pane_id)? else {
            self.last_agent_pane_id = None;
            return Ok(InsertResult::NoAgent);
        };
        if agent.workspace_id != self.workspace_id {
            self.last_agent_pane_id = None;
            return Ok(InsertResult::NoAgent);
        }

        let screen = client.read_agent_screen(&agent.pane_id)?;
        let text = AgentInputMode::detect(&screen).prepare(text);
        client.send_text(&agent.pane_id, &text)?;
        client.focus_agent(&agent.pane_id)?;
        Ok(InsertResult::Inserted {
            agent_name: agent
                .name
                .or(agent.display_agent)
                .unwrap_or(agent.pane_id.0),
        })
    }
}

fn missing_context(field: &'static str) -> crate::Error {
    crate::Error::Protocol {
        operation: "read Herdr plugin context".to_owned(),
        detail: field,
    }
}
