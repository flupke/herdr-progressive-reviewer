use std::sync::Mutex;

use super::*;

#[derive(Debug, Default)]
struct FakeReader {
    session: Mutex<SessionSnapshot>,
    agents: Mutex<Vec<Agent>>,
}

impl HerdrReader for FakeReader {
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

    fn list_plugin_panes(&self, _workspace_id: &WorkspaceId) -> Result<Vec<PluginPane>> {
        unreachable!()
    }
}

#[test]
fn focused_pane_remains_pending_until_herdr_detects_its_agent() {
    let reader = FakeReader::default();
    let workspace_id = workspace_id();
    let pane_id = PaneId("delayed-agent".to_owned());
    let mut target = AgentTarget::new(workspace_id, Some(pane_id.clone()));

    assert_eq!(target.resolve(&reader).unwrap(), None);

    reader
        .agents
        .lock()
        .unwrap()
        .push(agent(&pane_id.0, "workspace"));
    assert_eq!(
        target.resolve(&reader).unwrap().map(|agent| agent.pane_id),
        Some(pane_id)
    );
}

#[test]
fn most_recent_focused_agent_wins() {
    let reader = FakeReader::default();
    reader.agents.lock().unwrap().extend([
        agent("first-agent", "workspace"),
        agent("second-agent", "workspace"),
    ]);
    let mut target = AgentTarget::new(workspace_id(), None);
    target.observe_focus(&PaneId("first-agent".to_owned()));
    target.observe_focus(&PaneId("second-agent".to_owned()));

    assert_eq!(
        target.resolve(&reader).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("second-agent".to_owned()))
    );
}

#[test]
fn later_undetected_focus_replaces_the_target_after_detection() {
    let reader = FakeReader::default();
    reader
        .agents
        .lock()
        .unwrap()
        .push(agent("first-agent", "workspace"));
    let mut target = AgentTarget::new(workspace_id(), None);
    target.observe_focus(&PaneId("first-agent".to_owned()));
    target.observe_focus(&PaneId("delayed-agent".to_owned()));

    assert_eq!(
        target.resolve(&reader).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("first-agent".to_owned()))
    );

    reader
        .agents
        .lock()
        .unwrap()
        .push(agent("delayed-agent", "workspace"));
    assert_eq!(
        target.resolve(&reader).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("delayed-agent".to_owned()))
    );
}

#[test]
fn only_workspace_agent_is_selected_without_agent_focus_history() {
    let reader = FakeReader::default();
    reader
        .agents
        .lock()
        .unwrap()
        .push(agent("only-agent", "workspace"));
    let mut target = AgentTarget::new(
        workspace_id(),
        Some(PaneId("progressive-reviewer".to_owned())),
    );

    assert_eq!(
        target.resolve(&reader).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("only-agent".to_owned()))
    );
}

#[test]
fn only_agent_fallback_does_not_override_an_undetected_focused_agent() {
    let reader = FakeReader::default();
    reader
        .agents
        .lock()
        .unwrap()
        .push(agent("fallback-agent", "workspace"));
    let mut target = AgentTarget::new(workspace_id(), Some(PaneId("focused-agent".to_owned())));

    assert_eq!(
        target.resolve(&reader).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("fallback-agent".to_owned()))
    );

    reader
        .agents
        .lock()
        .unwrap()
        .push(agent("focused-agent", "workspace"));
    assert_eq!(
        target.resolve(&reader).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("focused-agent".to_owned()))
    );
}

#[test]
fn multiple_workspace_agents_are_not_selected_without_focus_history() {
    let reader = FakeReader::default();
    reader.agents.lock().unwrap().extend([
        agent("first-agent", "workspace"),
        agent("second-agent", "workspace"),
        agent("other-agent", "other-workspace"),
    ]);
    let mut target = AgentTarget::new(workspace_id(), None);

    assert_eq!(target.resolve(&reader).unwrap(), None);
}

#[test]
fn current_session_focus_replaces_the_previous_live_agent() {
    let reader = FakeReader::default();
    reader.agents.lock().unwrap().extend([
        agent("old-agent", "workspace"),
        agent("other-agent", "workspace"),
    ]);
    let mut target = AgentTarget::new(workspace_id(), None);
    target.observe_focus(&PaneId("old-agent".to_owned()));
    assert_eq!(
        target.resolve(&reader).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("old-agent".to_owned()))
    );

    *reader.session.lock().unwrap() = SessionSnapshot {
        focused_workspace_id: Some(workspace_id()),
        focused_pane_id: Some(PaneId("new-agent".to_owned())),
    };
    *reader.agents.lock().unwrap() = vec![
        agent("new-agent", "workspace"),
        agent("old-agent", "workspace"),
        agent("other-agent", "workspace"),
    ];

    assert_eq!(
        target.resolve(&reader).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("new-agent".to_owned()))
    );
}

#[test]
fn closed_agent_falls_back_to_the_previous_focused_live_agent() {
    let reader = FakeReader::default();
    reader.agents.lock().unwrap().extend([
        agent("first-agent", "workspace"),
        agent("second-agent", "workspace"),
        agent("third-agent", "workspace"),
    ]);
    let mut target = AgentTarget::new(workspace_id(), None);
    target.observe_focus(&PaneId("first-agent".to_owned()));
    target.observe_focus(&PaneId("second-agent".to_owned()));
    target.observe_focus(&PaneId("third-agent".to_owned()));
    assert_eq!(
        target.resolve(&reader).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("third-agent".to_owned()))
    );

    reader
        .agents
        .lock()
        .unwrap()
        .retain(|agent| agent.pane_id.0 != "third-agent");

    assert_eq!(
        target.resolve(&reader).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("second-agent".to_owned()))
    );
}

fn workspace_id() -> WorkspaceId {
    WorkspaceId("workspace".to_owned())
}

fn agent(pane_id: &str, workspace_id: &str) -> Agent {
    Agent {
        pane_id: PaneId(pane_id.to_owned()),
        tab_id: TabId("tab".to_owned()),
        workspace_id: WorkspaceId(workspace_id.to_owned()),
        name: None,
        display_agent: None,
        agent: None,
        agent_status: AgentStatus::Idle,
        agent_session: None,
        cwd: None,
    }
}
