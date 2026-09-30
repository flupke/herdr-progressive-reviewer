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
fn a_missing_session_waits_and_a_replacement_fails() {
    let port = InMemoryAgents::default();
    let pinned = PinnedAgent::new(agent(Some("original")));
    port.upsert_agent(agent(None));
    assert_eq!(pinned.current(&port), Ok(None));
    port.upsert_agent(agent(Some("original")));
    assert_eq!(pinned.current(&port), Ok(Some(agent(Some("original")))));
    port.upsert_agent(agent(Some("replacement")));
    assert_eq!(
        pinned.current(&port),
        Err(
            "The selected pane is now running a different agent conversation; start a new pass"
                .into()
        )
    );
}

#[test]
fn a_different_agent_in_the_pane_fails() {
    let port = InMemoryAgents::default();
    let pinned = PinnedAgent::new(agent(Some("original")));
    let mut other = agent(Some("original"));
    other.agent = Some("claude".into());
    port.upsert_agent(other);
    assert_eq!(
        pinned.current(&port),
        Err("The selected pane is now running a different agent; start a new pass".into())
    );
    port.remove_agent(&pane());
    assert_eq!(
        pinned.current(&port),
        Err("The selected agent is no longer available".into())
    );
}

#[test]
fn a_retry_without_a_session_is_bound_to_its_process() {
    let port = InMemoryAgents::default();
    assert_eq!(
        PinnedAgent::for_retry(agent(None), &port).err(),
        Some("Waiting for the selected agent process identity".into())
    );
    port.set_process_group(&pane(), 42);
    let pinned = PinnedAgent::for_retry(agent(None), &port).unwrap();
    port.upsert_agent(agent(None));
    assert_eq!(pinned.current(&port), Ok(Some(agent(None))));
    let changed = "The selected agent process changed; retry the interrupted turn";
    port.set_process_group(&pane(), 0);
    assert_eq!(pinned.current(&port), Err(changed.into()));
    port.set_process_group(&pane(), 43);
    assert_eq!(pinned.current(&port), Err(changed.into()));
}

#[test]
fn a_selected_prompt_follows_the_pane_until_sealed() {
    let port = InMemoryAgents::default();
    let pinned = PinnedAgent::for_selected_prompt(agent(Some("original")));
    port.upsert_agent(agent(Some("replacement")));
    assert_eq!(pinned.current(&port), Ok(Some(agent(Some("replacement")))));
    pinned.seal_attempt().unwrap();
    assert_eq!(pinned.known_agent(), Some(agent(Some("replacement"))));
    port.upsert_agent(agent(Some("original")));
    assert!(pinned.current(&port).is_err());
}
