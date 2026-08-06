use super::{EventEnvelope, FocusEvent};

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
