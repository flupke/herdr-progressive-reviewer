use herdr_client::memory::InMemoryAgents;
use herdr_client::protocol::AgentStatus;

use super::*;

const GROUP: u32 = 42;

fn session(value: &str) -> AgentSession {
    AgentSession {
        source: "herdr:codex".into(),
        agent: "codex".into(),
        kind: "id".into(),
        value: value.into(),
    }
}

fn agent(session: Option<AgentSession>) -> Agent {
    let mut agent: Agent = serde_json::from_value(serde_json::json!({
        "pane_id": "pane", "tab_id": "tab", "workspace_id": "workspace",
        "agent": "codex", "agent_status": "idle",
    }))
    .unwrap();
    agent.agent_session = session;
    agent
}

fn port() -> InMemoryAgents {
    let port = InMemoryAgents::default();
    port.set_process_group(&PaneId("pane".into()), GROUP);
    port
}

fn with_session(mut agent: Agent, value: Option<&str>) -> Agent {
    agent.agent_session = value.map(session);
    agent
}

fn process_identity(rules: IdentityRules, port: &InMemoryAgents) -> AgentIdentity {
    AgentIdentity::new(&agent(None), rules)
        .bind_process_without_session(port)
        .unwrap()
}

#[test]
fn a_resumed_session_is_the_same_agent_session() {
    let port = port();
    for rules in [IdentityRules::REVIEW_ACCESS, IdentityRules::PINNED_AGENT] {
        let original = agent(Some(session("original")));
        let mut identity = AgentIdentity::new(&original, rules);
        let mut resumed = original.clone();
        resumed.agent_status = AgentStatus::Working;
        assert_eq!(identity.check(&port, &resumed), Ok(Verdict::Same));
    }
}

#[test]
fn a_different_session_is_a_changed_session() {
    let port = port();
    for rules in [IdentityRules::REVIEW_ACCESS, IdentityRules::PINNED_AGENT] {
        let original = agent(Some(session("original")));
        let mut identity = AgentIdentity::new(&original, rules);
        let replacement = with_session(original, Some("replacement"));
        assert_eq!(
            identity.check(&port, &replacement),
            Ok(Verdict::Changed(Change::Session))
        );
    }
}

#[test]
fn a_different_pane_workspace_or_agent_is_a_changed_agent() {
    let port = port();
    for rules in [IdentityRules::REVIEW_ACCESS, IdentityRules::PINNED_AGENT] {
        let original = agent(Some(session("original")));
        let mut identity = AgentIdentity::new(&original, rules);
        let mut pane = original.clone();
        pane.pane_id.0 = "another-pane".into();
        let mut workspace = original.clone();
        workspace.workspace_id.0 = "another-workspace".into();
        let mut implementation = original;
        implementation.agent = Some("claude".into());
        for other in [pane, workspace, implementation] {
            assert_eq!(
                identity.check(&port, &other),
                Ok(Verdict::Changed(Change::Agent))
            );
        }
    }
}

#[test]
fn a_changed_foreground_process_is_a_changed_process_group() {
    let port = port();
    for rules in [IdentityRules::REVIEW_ACCESS, IdentityRules::PINNED_AGENT] {
        let mut identity = process_identity(rules, &port);
        assert_eq!(identity.check(&port, &agent(None)), Ok(Verdict::Same));
        port.set_process_group(&PaneId("pane".into()), GROUP + 1);
        assert_eq!(
            identity.check(&port, &agent(None)),
            Ok(Verdict::Changed(Change::ProcessGroup))
        );
        port.set_process_group(&PaneId("pane".into()), GROUP);
    }
}

#[test]
fn an_unidentified_foreground_process_is_an_error() {
    let port = InMemoryAgents::default();
    assert_eq!(
        AgentIdentity::new(&agent(None), IdentityRules::REVIEW_ACCESS)
            .bind_process_without_session(&port)
            .err(),
        Some(IdentityError::ProcessUnidentified)
    );
    let bound = self::port();
    let mut identity = process_identity(IdentityRules::REVIEW_ACCESS, &bound);
    bound.set_process_group(&PaneId("pane".into()), 0);
    assert_eq!(
        identity.check(&bound, &agent(None)),
        Err(IdentityError::ProcessUnidentified)
    );
}

#[test]
fn a_process_identity_adopts_the_first_session_and_rejects_a_replacement() {
    let port = port();
    for rules in [IdentityRules::REVIEW_ACCESS, IdentityRules::PINNED_AGENT] {
        let mut identity = process_identity(rules, &port);
        let base = agent(None);
        assert_eq!(identity.check(&port, &base), Ok(Verdict::Same));
        let resumed = with_session(base.clone(), Some("resumed"));
        assert_eq!(identity.check(&port, &resumed), Ok(Verdict::Same));
        assert_eq!(identity.check(&port, &resumed), Ok(Verdict::Same));
        assert_eq!(
            identity.check(&port, &with_session(base.clone(), Some("replacement"))),
            Ok(Verdict::Changed(Change::Session))
        );
        assert_eq!(identity.check(&port, &base), Ok(Verdict::SessionMissing));
    }
}

#[test]
fn review_access_keeps_checking_the_process_after_adopting_a_session() {
    let port = port();
    let mut identity = process_identity(IdentityRules::REVIEW_ACCESS, &port);
    let resumed = with_session(agent(None), Some("resumed"));
    assert_eq!(identity.check(&port, &resumed), Ok(Verdict::Same));
    port.set_process_group(&PaneId("pane".into()), GROUP + 1);
    assert_eq!(
        identity.check(&port, &resumed),
        Ok(Verdict::Changed(Change::ProcessGroup))
    );
}

#[test]
fn a_pinned_agent_releases_the_process_once_a_session_appears() {
    let port = port();
    let mut identity = process_identity(IdentityRules::PINNED_AGENT, &port);
    let resumed = with_session(agent(None), Some("resumed"));
    assert_eq!(identity.check(&port, &resumed), Ok(Verdict::Same));
    port.set_process_group(&PaneId("pane".into()), GROUP + 1);
    assert_eq!(identity.check(&port, &resumed), Ok(Verdict::Same));
}

#[test]
fn only_review_access_compares_the_session_source() {
    let port = port();
    let original = agent(Some(session("original")));
    let mut reported_elsewhere = original.clone();
    reported_elsewhere.agent_session.as_mut().unwrap().source = "herdr:other".into();
    let mut access = AgentIdentity::new(&original, IdentityRules::REVIEW_ACCESS);
    let mut pinned = AgentIdentity::new(&original, IdentityRules::PINNED_AGENT);
    assert_eq!(
        access.check(&port, &reported_elsewhere),
        Ok(Verdict::Changed(Change::Session))
    );
    assert_eq!(pinned.check(&port, &reported_elsewhere), Ok(Verdict::Same));
}

#[test]
fn a_following_identity_accepts_any_session_until_sealed() {
    let port = port();
    let original = agent(Some(session("original")));
    let mut identity = AgentIdentity::new(&original, IdentityRules::PINNED_AGENT).following();
    let replacement = with_session(original.clone(), Some("replacement"));
    assert_eq!(identity.check(&port, &agent(None)), Ok(Verdict::Same));
    assert_eq!(identity.check(&port, &replacement), Ok(Verdict::Same));
    identity.seal();
    assert_eq!(identity.check(&port, &replacement), Ok(Verdict::Same));
    assert_eq!(
        identity.check(&port, &original),
        Ok(Verdict::Changed(Change::Session))
    );
    assert_eq!(
        identity.check(&port, &agent(None)),
        Ok(Verdict::SessionMissing)
    );
}

#[test]
fn a_following_identity_seals_the_latest_observation_even_without_a_session() {
    let port = port();
    let original = agent(Some(session("original")));
    let mut identity = AgentIdentity::new(&original, IdentityRules::PINNED_AGENT).following();
    assert_eq!(identity.check(&port, &agent(None)), Ok(Verdict::Same));
    identity.seal();
    let replacement = with_session(original, Some("replacement"));
    assert_eq!(identity.check(&port, &replacement), Ok(Verdict::Same));
}

#[test]
fn a_following_identity_sealed_without_a_session_adopts_the_next_one() {
    let port = port();
    let mut identity = AgentIdentity::new(&agent(None), IdentityRules::PINNED_AGENT).following();
    identity.seal();
    let resumed = with_session(agent(None), Some("resumed"));
    assert_eq!(identity.check(&port, &resumed), Ok(Verdict::Same));
    assert_eq!(
        identity.check(&port, &with_session(agent(None), Some("replacement"))),
        Ok(Verdict::Changed(Change::Session))
    );
}
