//! Open, close, and toggle actions for the review pane.

use herdr_client::protocol::{
    EntrypointId, HerdrReader, HerdrWriter, OpenPluginPane, PaneId, PanePlacement, PluginContext,
    PluginPane, WorkspaceId,
};
use review_repository::repository::Repository;

const REVIEW_ENTRYPOINT: &str = "review";

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
    pub fn run(
        &self,
        action: PaneAction,
        context: &PluginContext,
    ) -> eyre::Result<PaneActionResult> {
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

    fn open(
        &self,
        context: &PluginContext,
        panes: &[PluginPane],
    ) -> eyre::Result<PaneActionResult> {
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

    fn close(&self, panes: Vec<PluginPane>) -> eyre::Result<PaneActionResult> {
        if panes.is_empty() {
            return Ok(PaneActionResult::AlreadyClosed);
        }
        for pane in panes {
            self.client.close_plugin_pane(&pane.pane_id)?;
        }
        Ok(PaneActionResult::Closed)
    }

    fn review_panes(&self, workspace: &WorkspaceId) -> eyre::Result<Vec<PluginPane>> {
        Ok(self
            .client
            .list_plugin_panes(workspace)?
            .into_iter()
            .filter(|pane| pane.entrypoint_id.0 == REVIEW_ENTRYPOINT)
            .collect())
    }

    fn close_duplicates(&self, panes: &[PluginPane]) -> eyre::Result<()> {
        for pane in panes.iter().skip(1) {
            self.client.close_plugin_pane(&pane.pane_id)?;
        }
        Ok(())
    }
}

fn missing_context(field: &'static str) -> herdr_client::Error {
    herdr_client::Error::Protocol {
        operation: "read Herdr plugin context".to_owned(),
        detail: field,
    }
}
