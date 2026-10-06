use std::io::{BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;

use super::{
    ApiError, EventCanceller, HerdrClient, HerdrEventStream, PaneId, Response, read_line,
    validate_response,
};
use crate::Error;
use crate::protocol::{AgentStatus, EntrypointId, HerdrEvent, PluginPane, TabId, WorkspaceId};

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
    let tie = EventCanceller::default().tie(&reader).unwrap().unwrap();
    HerdrEventStream {
        reader: BufReader::new(reader),
        tie,
    }
}

#[test]
fn forwards_focus_and_detection_without_restarting_for_new_agents() {
    let mut stream = event_stream(&[
        r#"{"event":"pane_focused","data":{"pane_id":"w1:p1","workspace_id":"w1","future":true}}"#,
        r#"{"event":"pane_agent_status_changed","data":{"pane_id":"w1:p1","agent_status":"working"}}"#,
        r#"{"event":"pane_agent_detected","data":{"pane_id":"w1:p1","workspace_id":"w1","released":true}}"#,
        r#"{"event":"pane_agent_detected","data":{"pane_id":"w1:p2","workspace_id":"w1","agent":"codex","released":false}}"#,
    ]);
    let mut received = Vec::new();
    stream
        .forward_while(|event| {
            received.push(event);
            received.len() < 3
        })
        .unwrap();
    assert_eq!(received[0], HerdrEvent::PaneFocused(PaneId("w1:p1".into())));
    assert!(matches!(
        &received[1],
        HerdrEvent::AgentDetected { released: true, .. }
    ));
    assert!(
        matches!(&received[2], HerdrEvent::AgentDetected { pane_id, agent, released: false, .. }
        if pane_id.0 == "w1:p2" && agent.as_deref() == Some("codex"))
    );
}

#[test]
fn a_disconnected_receiver_ends_the_event_stream() {
    let mut stream = event_stream(&[r#"{"event":"pane_focused","data":{"pane_id":"w1:p1"}}"#]);
    let (sender, receiver) = mpsc::channel();
    drop(receiver);
    stream
        .forward_while(|event| sender.send(event).is_ok())
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

#[test]
fn an_agent_status_stream_reads_each_status_and_skips_other_events() {
    let mut stream = event_stream(&[
        r#"{"event":"pane.agent_status_changed","data":{"pane_id":"w1:p1","workspace_id":"w1","agent_status":"working"}}"#,
        r#"{"event":"pane_focused","data":{"pane_id":"w1:p1"}}"#,
        r#"{"event":"pane_agent_status_changed","data":{"pane_id":"w1:p1","workspace_id":"w1","agent_status":"idle"}}"#,
    ]);

    assert_eq!(stream.next_status().unwrap(), Some(AgentStatus::Working));
    assert_eq!(stream.next_status().unwrap(), Some(AgentStatus::Idle));
    assert!(stream.next_status().is_err(), "Herdr closed the stream");
}

/// A stream whose writer stays open, as Herdr keeps an idle subscription, tied to
/// `canceller`, and its writer.
fn idle_stream(canceller: &EventCanceller) -> (HerdrEventStream, UnixStream) {
    let (reader, writer) = UnixStream::pair().unwrap();
    let tie = canceller.tie(&reader).unwrap().unwrap();
    let stream = HerdrEventStream {
        reader: BufReader::new(reader),
        tie,
    };
    (stream, writer)
}

#[test]
fn cancelling_ends_every_stream_that_waits_for_an_event() {
    let canceller = EventCanceller::default();
    let (mut first, _first_writer) = idle_stream(&canceller);
    let (mut second, _second_writer) = idle_stream(&canceller);
    let first = std::thread::spawn(move || first.forward_while(|_| true));
    let second = std::thread::spawn(move || second.next_status());

    canceller.cancel();

    first.join().unwrap().unwrap();
    assert_eq!(second.join().unwrap().unwrap(), None);
}

#[test]
fn a_stream_that_ends_lets_go_of_its_socket() {
    let canceller = EventCanceller::default();
    let (stream, mut writer) = idle_stream(&canceller);

    drop(stream);

    assert!(canceller.lock().sockets.is_empty());
    // The stream's socket is closed: its peer reads the end.
    assert_eq!(std::io::Read::read(&mut writer, &mut [0; 1]).unwrap(), 0);
}

#[test]
fn a_cancelled_canceller_refuses_the_next_subscription() {
    let (socket, _peer) = UnixStream::pair().unwrap();
    let canceller = EventCanceller::default();
    canceller.cancel();

    assert!(canceller.tie(&socket).unwrap().is_none());
}

#[test]
fn a_prompt_waits_as_long_as_the_client_says_and_a_timeout_is_an_agent_that_did_not_start() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("socket");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let herdr = std::thread::spawn(move || {
        let (connection, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(connection.try_clone().unwrap());
        let request: serde_json::Value =
            serde_json::from_slice(&read_line(&mut reader, "test").unwrap()).unwrap();
        writeln!(
            &connection,
            r#"{{"id":"progressive-reviewer","error":{{"code":"timeout","message":"no activity"}}}}"#
        )
        .unwrap();
        request
    });
    let client = HerdrClient::new(socket, "plugin".to_owned(), directory.path().join("state"))
        .with_prompt_start_timeout(std::time::Duration::from_millis(300));

    let prompted = crate::protocol::AgentPort::prompt_agent(&client, &PaneId("p".into()), "hi");

    assert!(matches!(prompted, Err(Error::AgentNotStarted { .. })));
    assert_eq!(herdr.join().unwrap()["params"]["wait"]["timeout_ms"], 300);
}
