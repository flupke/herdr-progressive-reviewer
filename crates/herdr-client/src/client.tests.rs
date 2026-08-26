use serde_json::json;

use super::{
    AgentDetectedEvent, ApiError, EventEnvelope, FocusEvent, PaneId, Response, event_subscriptions,
    validate_response,
};

#[test]
fn reads_the_herdr_focus_event_envelope() {
    let event: EventEnvelope = serde_json::from_str(
        r#"{"event":"pane_focused","data":{"type":"pane_focused","pane_id":"w1:p2","workspace_id":"w1","future":true}}"#,
    )
    .unwrap();
    let focus: FocusEvent = serde_json::from_value(event.data).unwrap();

    assert_eq!(event.event, "pane_focused");
    assert_eq!(focus.pane_id.0, "w1:p2");
}

#[test]
fn reads_an_agent_release_event() {
    let event: EventEnvelope = serde_json::from_str(
        r#"{"event":"pane_agent_detected","data":{"pane_id":"w1:p2","workspace_id":"w1","agent":"codex","released":true,"final_status":"done"}}"#,
    )
    .unwrap();
    let detected: AgentDetectedEvent = serde_json::from_value(event.data).unwrap();

    assert!(detected.released);
    assert_eq!(detected.agent.as_deref(), Some("codex"));
}

#[test]
fn scopes_agent_status_subscriptions_to_panes() {
    let subscriptions = event_subscriptions(&[PaneId("w1:p2".to_owned())]);

    assert_eq!(
        subscriptions,
        vec![
            json!({"type": "pane.focused"}),
            json!({"type": "pane.agent_detected"}),
            json!({"type": "pane.agent_status_changed", "pane_id": "w1:p2"}),
        ]
    );
}

#[test]
fn reports_a_rejected_event_subscription() {
    let response = Response {
        id: "progressive-reviewer-events".to_owned(),
        result: None,
        error: Some(ApiError {
            code: "invalid_params".to_owned(),
            message: "pane_id is required".to_owned(),
        }),
    };

    let error = validate_response(
        response,
        "progressive-reviewer-events",
        "subscribe to Herdr events",
    )
    .unwrap()
    .unwrap_err();

    assert_eq!(error.code, "invalid_params");
    assert_eq!(error.message, "pane_id is required");
}
