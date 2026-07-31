use std::sync::Mutex;

use pr_core::herdr::{
    Agent, AgentTarget, EntrypointId, HerdrReader, HerdrWriter, InsertResult, OpenPluginPane,
    PaneAction, PaneActionResult, PaneActions, PaneId, PanePlacement, PluginContext, PluginPane,
    SessionSnapshot, TabId, WorkspaceId,
};
use pr_core::{Error, Result};
use pr_tests::{JjFixture, JjLayout};

#[derive(Debug, Default)]
struct FakeHerdr {
    session: Mutex<SessionSnapshot>,
    agents: Mutex<Vec<Agent>>,
    agent_screen: Mutex<String>,
    panes: Mutex<Vec<PluginPane>>,
    opened: Mutex<Vec<OpenPluginPane>>,
    focused: Mutex<Vec<PaneId>>,
    closed: Mutex<Vec<PaneId>>,
    sent: Mutex<Vec<(PaneId, String)>>,
    race_on_open: Mutex<bool>,
    fail_send: Mutex<bool>,
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
        Ok(self.agent_screen.lock().unwrap().clone())
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

    fn send_text(&self, pane_id: &PaneId, text: &str) -> Result<()> {
        if *self.fail_send.lock().unwrap() {
            return Err(Error::Protocol {
                operation: "send text".to_owned(),
                detail: "fixture failure",
            });
        }
        self.sent
            .lock()
            .unwrap()
            .push((pane_id.clone(), text.to_owned()));
        Ok(())
    }
}

#[test]
fn pane_actions_are_idempotent_and_remove_a_racing_duplicate() {
    let fixture = JjFixture::new(JjLayout::NonColocated);
    let client = FakeHerdr::default();
    *client.race_on_open.lock().unwrap() = true;
    let context = PluginContext {
        workspace_id: Some(WorkspaceId("workspace".to_owned())),
        tab_id: Some(TabId("tab".to_owned())),
        focused_pane_id: Some(PaneId("agent".to_owned())),
        focused_pane_cwd: Some(fixture.root().to_owned()),
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
            cwd: fixture.root().to_owned(),
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

#[test]
fn insertion_rechecks_the_last_focused_agent_workspace() {
    let client = FakeHerdr::default();
    let workspace = WorkspaceId("workspace".to_owned());
    let first_agent = agent("agent-1", "workspace", Some("Codex"));
    *client.session.lock().unwrap() = SessionSnapshot {
        focused_workspace_id: Some(workspace.clone()),
        focused_pane_id: Some(first_agent.pane_id.clone()),
    };
    client.agents.lock().unwrap().push(first_agent);
    *client.agent_screen.lock().unwrap() = "prompt\nVim: Normal\n".to_owned();
    let mut target = AgentTarget::new(workspace);
    target.initialize(&client).unwrap();

    assert_eq!(
        target.insert(&client, "diff text").unwrap(),
        InsertResult::Inserted {
            agent_name: "Codex".to_owned(),
        }
    );
    assert_eq!(
        client.sent.lock().unwrap().as_slice(),
        [(PaneId("agent-1".to_owned()), "idiff text".to_owned())]
    );
    assert_eq!(
        client.focused.lock().unwrap().as_slice(),
        [PaneId("agent-1".to_owned())]
    );

    target.observe_agent_focus(&agent("other-agent", "other", None));
    *client.agent_screen.lock().unwrap() = "prompt\nVim: Insert\n".to_owned();
    assert!(matches!(
        target.insert(&client, "same target").unwrap(),
        InsertResult::Inserted { .. }
    ));
    client.agents.lock().unwrap()[0].workspace_id = WorkspaceId("other".to_owned());
    assert_eq!(
        target.insert(&client, "must not send").unwrap(),
        InsertResult::NoAgent
    );
    assert_eq!(client.sent.lock().unwrap().len(), 2);

    let agent = agent("agent-2", "workspace", None);
    client.agents.lock().unwrap().push(agent.clone());
    target.observe_agent_focus(&agent);
    *client.fail_send.lock().unwrap() = true;
    assert!(target.insert(&client, "retry").is_err());
    *client.fail_send.lock().unwrap() = false;
    assert_eq!(
        target.insert(&client, "retry").unwrap(),
        InsertResult::Inserted {
            agent_name: "agent-2".to_owned(),
        }
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

fn agent(id: &str, workspace: &str, name: Option<&str>) -> Agent {
    Agent {
        pane_id: PaneId(id.to_owned()),
        tab_id: TabId("tab".to_owned()),
        workspace_id: WorkspaceId(workspace.to_owned()),
        name: name.map(str::to_owned),
        display_agent: None,
        cwd: None,
    }
}
