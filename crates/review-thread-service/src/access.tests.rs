use herdr_client::memory::InMemoryAgents;
use herdr_client::protocol::{AgentSession, PaneId};

use super::*;

fn agent(session: Option<&str>) -> Agent {
    let mut agent: Agent = serde_json::from_value(serde_json::json!({
        "pane_id": "pane", "tab_id": "tab", "workspace_id": "workspace",
        "agent": "codex", "agent_status": "idle",
    }))
    .unwrap();
    agent.agent_session = session.map(|value| AgentSession {
        source: "herdr:codex".into(),
        agent: "codex".into(),
        kind: "id".into(),
        value: value.into(),
    });
    agent
}

fn pane() -> PaneId {
    PaneId("pane".into())
}

#[test]
fn clones_share_the_session_adopted_by_a_process_grant() {
    let port = InMemoryAgents::default();
    port.set_process_group(&pane(), 42);
    let access = Access::new("review".into(), agent(None), &port).unwrap();
    let clone = access.clone();
    assert!(clone.matches_agent(&port, &agent(Some("resumed"))).unwrap());
    assert!(
        !access
            .matches_agent(&port, &agent(Some("replacement")))
            .unwrap()
    );
    assert!(!access.matches_agent(&port, &agent(None)).unwrap());
}

#[test]
fn an_unidentified_process_is_reported_by_review_access() {
    let port = InMemoryAgents::default();
    let error = "Herdr did not identify the selected agent process";
    assert_eq!(
        Access::new("review".into(), agent(None), &port).err(),
        Some(error.to_owned())
    );
    port.set_process_group(&pane(), 42);
    let access = Access::new("review".into(), agent(None), &port).unwrap();
    port.set_process_group(&pane(), 0);
    assert_eq!(access.matches_agent(&port, &agent(None)), Err(error.into()));
}

#[test]
fn a_replaced_or_exited_agent_invalidates_the_grant() {
    let port = InMemoryAgents::default();
    let access = Access::new("review".into(), agent(Some("original")), &port).unwrap();
    port.upsert_agent(agent(Some("original")));
    assert!(access.current_agent(&port).is_ok());
    port.upsert_agent(agent(Some("replacement")));
    assert!(
        access
            .current_agent(&port)
            .unwrap_err()
            .starts_with("The selected pane has changed agent processes or sessions")
    );
    port.remove_agent(&pane());
    assert_eq!(
        access.current_agent(&port).err(),
        Some("The selected agent has exited".into())
    );
}
