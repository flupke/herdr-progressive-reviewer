use std::io::{BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;

use serde_json::json;

use super::{
    AgentDetectedEvent, ApiError, EventEnvelope, FocusEvent, HerdrClient, HerdrEventStream, PaneId,
    Response, event_subscriptions, read_line, validate_response,
};
use crate::Error;
use crate::protocol::{EntrypointId, HerdrEvent, PluginPane, TabId, WorkspaceId};

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
fn subscribes_to_focus_and_agent_detection() {
    let subscriptions = event_subscriptions();

    assert_eq!(
        subscriptions,
        vec![
            json!({"type": "pane.focused"}),
            json!({"type": "pane.agent_detected"}),
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

fn event_stream(lines: &[&str]) -> HerdrEventStream {
    let (reader, mut writer) = UnixStream::pair().unwrap();
    for line in lines {
        writeln!(writer, "{line}").unwrap();
    }
    drop(writer);
    HerdrEventStream {
        reader: BufReader::new(reader),
    }
}

#[test]
fn forwards_focus_and_detection_without_restarting_for_new_agents() {
    let mut stream = event_stream(&[
        r#"{"event":"pane_focused","data":{"pane_id":"w1:p1"}}"#,
        r#"{"event":"pane_agent_status_changed","data":{"pane_id":"w1:p1","agent_status":"working"}}"#,
        r#"{"event":"pane_agent_detected","data":{"pane_id":"w1:p1","workspace_id":"w1","released":true}}"#,
        r#"{"event":"pane_agent_detected","data":{"pane_id":"w1:p2","workspace_id":"w1","agent":"codex","released":false}}"#,
    ]);
    let mut received = Vec::new();
    stream
        .forward_while(
            || true,
            |event| {
                received.push(event);
                received.len() < 3
            },
        )
        .unwrap();
    assert_eq!(received[0], HerdrEvent::PaneFocused(PaneId("w1:p1".into())));
    assert!(matches!(
        &received[1],
        HerdrEvent::AgentDetected { released: true, .. }
    ));
    assert!(
        matches!(&received[2], HerdrEvent::AgentDetected { pane_id, released: false, .. }
        if pane_id.0 == "w1:p2")
    );
}

#[test]
fn a_disconnected_receiver_ends_the_event_stream() {
    let mut stream = event_stream(&[r#"{"event":"pane_focused","data":{"pane_id":"w1:p1"}}"#]);
    let (sender, receiver) = mpsc::channel();
    drop(receiver);
    stream
        .forward_while(|| true, |event| sender.send(event).is_ok())
        .unwrap();
}

#[test]
fn pane_records_round_trip_and_remove_by_pane_id() {
    let directory = tempfile::tempdir().unwrap();
    let client = HerdrClient::new(
        directory.path().join("socket"),
        "plugin".to_owned(),
        directory.path().join("state"),
    );
    let pane = PluginPane {
        pane_id: PaneId("pane".to_owned()),
        tab_id: TabId("tab".to_owned()),
        workspace_id: WorkspaceId("w/λ".to_owned()),
        entrypoint_id: EntrypointId("review".to_owned()),
    };

    assert_eq!(client.load_pane(&pane.workspace_id).unwrap(), None);
    client.save_pane(&pane).unwrap();
    assert_eq!(
        client.load_pane(&pane.workspace_id).unwrap(),
        Some(pane.clone())
    );
    let record = client.pane_path(&pane.workspace_id);
    assert_eq!(record.parent().unwrap().file_name().unwrap(), "panes");
    assert!(
        record
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".json")
    );
    assert_eq!(
        std::fs::metadata(record.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    client.remove_pane(&pane.pane_id).unwrap();
    assert_eq!(client.load_pane(&pane.workspace_id).unwrap(), None);
    client.remove_pane(&pane.pane_id).unwrap();
}

#[test]
fn pane_record_read_errors_are_not_treated_as_missing() {
    let directory = tempfile::tempdir().unwrap();
    let client = HerdrClient::new(
        directory.path().join("socket"),
        "plugin".to_owned(),
        directory.path().join("state"),
    );
    let workspace = WorkspaceId("workspace".to_owned());
    std::fs::create_dir_all(client.pane_path(&workspace)).unwrap();
    assert!(matches!(
        client.load_pane(&workspace),
        Err(Error::Io { .. })
    ));
}

#[test]
fn removing_a_pane_succeeds_when_the_state_directory_is_missing() {
    let directory = tempfile::tempdir().unwrap();
    let client = HerdrClient::new(
        directory.path().join("socket"),
        "plugin".to_owned(),
        directory.path().join("missing-state"),
    );
    client.remove_pane(&PaneId("pane".to_owned())).unwrap();
}

#[test]
fn line_reader_accepts_the_limit_and_rejects_one_extra_byte() {
    assert_eq!(super::RESPONSE_LIMIT, 16_777_216);
    let response_limit = usize::try_from(super::RESPONSE_LIMIT).unwrap();
    let mut exact = std::io::Cursor::new(vec![b'x'; response_limit]);
    assert_eq!(
        read_line(&mut exact, "test").unwrap().len() as u64,
        super::RESPONSE_LIMIT
    );
    let mut large = std::io::Cursor::new(vec![b'x'; response_limit + 1]);
    assert!(matches!(
        read_line(&mut large, "test"),
        Err(Error::Protocol { .. })
    ));
}
