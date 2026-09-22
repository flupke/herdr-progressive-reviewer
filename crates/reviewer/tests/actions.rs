use std::sync::Mutex;

use herdr_client::Result;
use herdr_client::protocol::{
    Agent, EntrypointId, HerdrReader, HerdrWriter, OpenPluginPane, PaneId, PanePlacement,
    PluginContext, PluginPane, SessionSnapshot, TabId, WorkspaceId,
};
use review_repository::repository::RepoType;
use review_test_support::repository_fixture;
use reviewer::control::{PaneAction, PaneActionResult, PaneActions};
use test_case::test_case;

#[derive(Debug, Default)]
struct FakeHerdr {
    session: Mutex<SessionSnapshot>,
    agents: Mutex<Vec<Agent>>,
    panes: Mutex<Vec<PluginPane>>,
    opened: Mutex<Vec<OpenPluginPane>>,
    focused: Mutex<Vec<PaneId>>,
    closed: Mutex<Vec<PaneId>>,
    race_on_open: Mutex<bool>,
}

impl HerdrReader for FakeHerdr {
    fn session_snapshot(&self) -> Result<SessionSnapshot> {
        Ok(self.session.lock().unwrap().clone())
    }

    fn list_agents(&self) -> Result<Vec<Agent>> {
        Ok(self.agents.lock().unwrap().clone())
    }

    fn get_agent(&self, pane_id: &PaneId) -> Result<Option<Agent>> {
        Ok(self
            .agents
            .lock()
            .unwrap()
            .iter()
            .find(|agent| agent.pane_id == *pane_id)
            .cloned())
    }

    fn read_agent_screen(&self, _pane_id: &PaneId) -> Result<String> {
        unreachable!()
    }

    fn list_plugin_panes(&self, workspace_id: &WorkspaceId) -> Result<Vec<PluginPane>> {
        Ok(self
            .panes
            .lock()
            .unwrap()
            .iter()
            .filter(|pane| pane.workspace_id == *workspace_id)
            .cloned()
            .collect())
    }
}

impl HerdrWriter for FakeHerdr {
    fn open_plugin_pane(&self, request: &OpenPluginPane) -> Result<PluginPane> {
        self.opened.lock().unwrap().push(request.clone());
        let mut panes = self.panes.lock().unwrap();
        if std::mem::take(&mut *self.race_on_open.lock().unwrap()) {
            panes.push(review_pane("review-first"));
        }
        let pane = review_pane("review-new");
        panes.push(pane.clone());
        Ok(pane)
    }

    fn focus_plugin_pane(&self, pane_id: &PaneId) -> Result<()> {
        self.focused.lock().unwrap().push(pane_id.clone());
        Ok(())
    }

    fn focus_agent(&self, pane_id: &PaneId) -> Result<()> {
        self.focused.lock().unwrap().push(pane_id.clone());
        Ok(())
    }

    fn close_plugin_pane(&self, pane_id: &PaneId) -> Result<()> {
        self.closed.lock().unwrap().push(pane_id.clone());
        self.panes
            .lock()
            .unwrap()
            .retain(|pane| pane.pane_id != *pane_id);
        Ok(())
    }
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn pane_actions_are_idempotent_and_remove_a_racing_duplicate(repository_type: RepoType) {
    let repository = repository_fixture(repository_type);
    let client = FakeHerdr::default();
    *client.race_on_open.lock().unwrap() = true;
    let context = PluginContext {
        workspace_id: Some(WorkspaceId("workspace".to_owned())),
        tab_id: Some(TabId("tab".to_owned())),
        focused_pane_id: Some(PaneId("agent".to_owned())),
        focused_pane_cwd: Some(repository.root().to_owned()),
    };
    let actions = PaneActions::new(&client);

    assert_eq!(
        actions.run(PaneAction::Open, &context).unwrap(),
        PaneActionResult::Focused(PaneId("review-first".to_owned()))
    );
    assert_eq!(
        client.opened.lock().unwrap().as_slice(),
        [OpenPluginPane {
            entrypoint: EntrypointId("review".to_owned()),
            placement: PanePlacement::Split,
            target_pane_id: PaneId("agent".to_owned()),
            cwd: repository.root().to_owned(),
            focus: true,
        }]
    );
    assert_eq!(
        client.closed.lock().unwrap().as_slice(),
        [PaneId("review-new".to_owned())]
    );
    assert_eq!(
        actions.run(PaneAction::Open, &context).unwrap(),
        PaneActionResult::Focused(PaneId("review-first".to_owned()))
    );
    assert_eq!(client.opened.lock().unwrap().len(), 1);

    assert_eq!(
        actions.run(PaneAction::Toggle, &context).unwrap(),
        PaneActionResult::Closed
    );
    assert_eq!(
        actions.run(PaneAction::Close, &context).unwrap(),
        PaneActionResult::AlreadyClosed
    );
}

fn review_pane(id: &str) -> PluginPane {
    PluginPane {
        pane_id: PaneId(id.to_owned()),
        tab_id: TabId("tab".to_owned()),
        workspace_id: WorkspaceId("workspace".to_owned()),
        entrypoint_id: EntrypointId("review".to_owned()),
    }
}
