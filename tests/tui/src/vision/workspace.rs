//! The session's workspace in the user's Herdr. The session closes it only while it is still
//! the session's: the label it gave it, and no pane the session did not open, so a workspace the
//! user took over, or one that reuses the ID of a workspace the user closed, stays open.

use std::path::Path;

use anyhow::{Context, Result};
use herdr_client::client::HerdrClient;
use serde_json::{Value, json};

/// The label of a session's workspace.
pub(crate) const LABEL: &str = "reviewer vision";

/// The panes of the session's tab.
pub(crate) struct Panes {
    pub(crate) tab: String,
    pub(crate) reviewer: String,
    pub(crate) agent: String,
}

/// What closing the session's workspace did.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Closing {
    Closed,
    /// The workspace was already gone.
    AlreadyGone,
    /// The workspace is no longer the session's alone, so it stays open.
    LeftOpen(String),
}

impl Closing {
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Closed => "closed".into(),
            Self::AlreadyGone => "already closed".into(),
            Self::LeftOpen(reason) => format!("left open: {reason}"),
        }
    }
}

pub(crate) struct Workspace {
    herdr: HerdrClient,
    id: String,
    /// The panes the session opened in the workspace and that may still be there.
    opened: Vec<String>,
    open: bool,
}

impl Workspace {
    /// Create the workspace on `cwd`, and return it with its first tab.
    pub(crate) fn create(herdr: &HerdrClient, cwd: &Path, focus: bool) -> Result<(Self, String)> {
        let created = herdr.request(
            "workspace.create",
            &json!({"cwd": cwd, "label": LABEL, "focus": focus}),
        )?;
        let workspace = Self {
            herdr: herdr.clone(),
            id: string(&created["workspace"]["workspace_id"])?,
            opened: vec![string(&created["root_pane"]["pane_id"])?],
            open: true,
        };
        Ok((workspace, string(&created["tab"]["tab_id"])?))
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn herdr(&self) -> &HerdrClient {
        &self.herdr
    }

    /// Record the panes that replaced the ones the session opened before.
    pub(crate) fn opened(&mut self, panes: &Panes) {
        self.opened = vec![panes.reviewer.clone(), panes.agent.clone()];
    }

    /// Close the workspace if it is still the session's.
    pub(crate) fn close(&mut self) -> Closing {
        if !std::mem::take(&mut self.open) {
            return Closing::AlreadyGone;
        }
        let closed = match self.foreign() {
            Ok(None) => self
                .herdr
                .request("workspace.close", &json!({"workspace_id": self.id}))
                .map_or_else(
                    |error| Closing::LeftOpen(error.to_string()),
                    |_| Closing::Closed,
                ),
            Ok(Some(reason)) => Closing::LeftOpen(reason),
            Err(Gone) => Closing::AlreadyGone,
        };
        if let Closing::LeftOpen(reason) = &closed {
            eprintln!("reviewer-vision: workspace {} {reason}", self.id);
        }
        closed
    }

    /// Why the workspace is no longer the session's alone, if it is not.
    fn foreign(&self) -> Result<Option<String>, Gone> {
        let workspace = self
            .herdr
            .request("workspace.get", &json!({"workspace_id": self.id}))
            .map_err(|_| Gone)?;
        let label = workspace["workspace"]["label"].as_str().unwrap_or_default();
        if label != LABEL {
            return Ok(Some(format!("its label is now {label:?}")));
        }
        let listed = match self
            .herdr
            .request("pane.list", &json!({"workspace_id": self.id}))
        {
            Ok(listed) => listed,
            Err(error) => return Ok(Some(format!("its panes cannot be listed: {error}"))),
        };
        let panes: Vec<&str> = listed["panes"]
            .as_array()
            .map(|panes| {
                panes
                    .iter()
                    .filter_map(|pane| pane["pane_id"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        if panes.is_empty() {
            return Ok(Some("it holds none of the session's panes".into()));
        }
        Ok(panes
            .iter()
            .find(|pane| !self.opened.iter().any(|opened| opened == *pane))
            .map(|pane| format!("it holds pane {pane}, which the session did not open")))
    }
}

/// The workspace no longer exists.
struct Gone;

impl Drop for Workspace {
    fn drop(&mut self) {
        self.close();
    }
}

pub(crate) fn string(value: &Value) -> Result<String> {
    Ok(value
        .as_str()
        .with_context(|| format!("Herdr answered without an expected value: {value}"))?
        .to_owned())
}
