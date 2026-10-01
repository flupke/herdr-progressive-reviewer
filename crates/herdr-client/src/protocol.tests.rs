use super::*;
use crate::memory::InMemoryAgents;

#[test]
fn focused_pane_remains_pending_until_herdr_detects_its_agent() {
    let agents = InMemoryAgents::default();
    let workspace_id = workspace_id();
    let pane_id = PaneId("delayed-agent".to_owned());
    let mut target = AgentTarget::new(workspace_id, Some(pane_id.clone()));

    assert_eq!(target.resolve(&agents).unwrap(), None);

    agents.upsert_agent(agent(&pane_id.0, "workspace"));
    assert_eq!(
        target.resolve(&agents).unwrap().map(|agent| agent.pane_id),
        Some(pane_id)
    );
}

#[test]
fn shared_target_handles_use_the_same_focus_history() {
    let agents = InMemoryAgents::default();
    agents.upsert_agent(agent("first-agent", "workspace"));
    agents.upsert_agent(agent("second-agent", "workspace"));
    let mut explore = AgentTarget::new(workspace_id(), Some(PaneId("first-agent".into())));
    let mut comments = explore.clone();
    comments.observe_focus(&PaneId("second-agent".into()));
    assert_eq!(
        explore.resolve(&agents).unwrap(),
        comments.resolve(&agents).unwrap()
    );
    assert_eq!(
        explore.resolve(&agents).unwrap().unwrap().pane_id.0,
        "second-agent"
    );
    explore.observe_focus(&PaneId("first-agent".into()));
    assert_eq!(
        comments.resolve(&agents).unwrap().unwrap().pane_id.0,
        "first-agent"
    );
}

#[test]
fn most_recent_focused_agent_wins() {
    let agents = InMemoryAgents::default();
    agents.upsert_agent(agent("first-agent", "workspace"));
    agents.upsert_agent(agent("second-agent", "workspace"));
    let mut target = AgentTarget::new(workspace_id(), None);
    target.observe_focus(&PaneId("first-agent".to_owned()));
    target.observe_focus(&PaneId("second-agent".to_owned()));

    assert_eq!(
        target.resolve(&agents).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("second-agent".to_owned()))
    );
}

#[test]
fn later_undetected_focus_replaces_the_target_after_detection() {
    let agents = InMemoryAgents::default();
    agents.upsert_agent(agent("first-agent", "workspace"));
    let mut target = AgentTarget::new(workspace_id(), None);
    target.observe_focus(&PaneId("first-agent".to_owned()));
    target.observe_focus(&PaneId("delayed-agent".to_owned()));

    assert_eq!(
        target.resolve(&agents).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("first-agent".to_owned()))
    );

    agents.upsert_agent(agent("delayed-agent", "workspace"));
    assert_eq!(
        target.resolve(&agents).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("delayed-agent".to_owned()))
    );
}

#[test]
fn only_workspace_agent_is_selected_without_agent_focus_history() {
    let agents = InMemoryAgents::default();
    agents.upsert_agent(agent("only-agent", "workspace"));
    let mut target = AgentTarget::new(
        workspace_id(),
        Some(PaneId("progressive-reviewer".to_owned())),
    );

    assert_eq!(
        target.resolve(&agents).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("only-agent".to_owned()))
    );
}

#[test]
fn only_agent_fallback_does_not_override_an_undetected_focused_agent() {
    let agents = InMemoryAgents::default();
    agents.upsert_agent(agent("fallback-agent", "workspace"));
    let mut target = AgentTarget::new(workspace_id(), Some(PaneId("focused-agent".to_owned())));

    assert_eq!(
        target.resolve(&agents).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("fallback-agent".to_owned()))
    );

    agents.upsert_agent(agent("focused-agent", "workspace"));
    assert_eq!(
        target.resolve(&agents).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("focused-agent".to_owned()))
    );
}

#[test]
fn multiple_workspace_agents_are_not_selected_without_focus_history() {
    let agents = InMemoryAgents::default();
    agents.upsert_agent(agent("first-agent", "workspace"));
    agents.upsert_agent(agent("second-agent", "workspace"));
    agents.upsert_agent(agent("other-agent", "other-workspace"));
    let mut target = AgentTarget::new(workspace_id(), None);

    assert_eq!(target.resolve(&agents).unwrap(), None);
}

#[test]
fn current_session_focus_replaces_the_previous_live_agent() {
    let agents = InMemoryAgents::default();
    agents.upsert_agent(agent("old-agent", "workspace"));
    agents.upsert_agent(agent("other-agent", "workspace"));
    let mut target = AgentTarget::new(workspace_id(), None);
    target.observe_focus(&PaneId("old-agent".to_owned()));
    assert_eq!(
        target.resolve(&agents).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("old-agent".to_owned()))
    );

    agents.set_session(SessionSnapshot {
        focused_workspace_id: Some(workspace_id()),
        focused_pane_id: Some(PaneId("new-agent".to_owned())),
    });
    agents.upsert_agent(agent("new-agent", "workspace"));

    assert_eq!(
        target.resolve(&agents).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("new-agent".to_owned()))
    );
}

#[test]
fn closed_agent_falls_back_to_the_previous_focused_live_agent() {
    let agents = InMemoryAgents::default();
    agents.upsert_agent(agent("first-agent", "workspace"));
    agents.upsert_agent(agent("second-agent", "workspace"));
    agents.upsert_agent(agent("third-agent", "workspace"));
    let mut target = AgentTarget::new(workspace_id(), None);
    target.observe_focus(&PaneId("first-agent".to_owned()));
    target.observe_focus(&PaneId("second-agent".to_owned()));
    target.observe_focus(&PaneId("third-agent".to_owned()));
    assert_eq!(
        target.resolve(&agents).unwrap().map(|agent| agent.pane_id),
        Some(PaneId("third-agent".to_owned()))
    );

    agents.remove_agent(&PaneId("third-agent".to_owned()));

    assert_eq!(
        target.resolve(&agents).unwrap().map(|agent| agent.pane_id),
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

#[test]
fn a_pane_splits_across_the_side_that_looks_longer() {
    let size = |width, height| PaneSize { width, height };

    assert_eq!(size(240, 60).split_direction(), SplitDirection::Right);
    assert_eq!(size(150, 60).split_direction(), SplitDirection::Right);
    assert_eq!(size(149, 60).split_direction(), SplitDirection::Down);
    // Half of a 320x80 screen: splitting it beside would leave three columns.
    assert_eq!(size(160, 78).split_direction(), SplitDirection::Down);
    assert_eq!(size(60, 75).split_direction(), SplitDirection::Down);
}

#[test]
fn a_pane_size_reads_from_a_herdr_layout_rect() {
    let size: PaneSize =
        serde_json::from_str(r#"{"height": 40, "width": 60, "x": 60, "y": 0}"#).unwrap();

    assert_eq!(
        size,
        PaneSize {
            width: 60,
            height: 40
        }
    );
}

#[test]
fn split_direction_names_match_the_wire() {
    for direction in [SplitDirection::Right, SplitDirection::Down] {
        assert_eq!(serde_json::to_value(direction).unwrap(), direction.as_str());
    }
}
