use std::io::{BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;

use test_case::test_case;

use super::{
    ApiError, EventCanceller, HerdrClient, HerdrEventStream, PaneId, Response, read_line,
    validate_response,
};
use crate::Error;
use crate::protocol::{
    AgentStatus, EntrypointId, HerdrEvent, HerdrReader, PluginPane, TabId, WorkspaceId,
};

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

    assert_eq!(client.load_pane(&pane.tab_id).unwrap(), None);
    client.save_pane(&pane).unwrap();
    assert_eq!(client.load_pane(&pane.tab_id).unwrap(), Some(pane.clone()));
    let record = client.pane_path(&pane.tab_id);
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
    assert_eq!(client.load_pane(&pane.tab_id).unwrap(), None);
    client.remove_pane(&pane.pane_id).unwrap();
}

#[test]
fn each_tab_of_a_workspace_keeps_its_own_pane_record() {
    let directory = tempfile::tempdir().unwrap();
    let client = HerdrClient::new(
        directory.path().join("socket"),
        "plugin".to_owned(),
        directory.path().join("state"),
    );
    let pane_in = |pane: &str, tab: &str| PluginPane {
        pane_id: PaneId(pane.to_owned()),
        tab_id: TabId(tab.to_owned()),
        workspace_id: WorkspaceId("w1".to_owned()),
        entrypoint_id: EntrypointId("review".to_owned()),
    };
    let lead = pane_in("w1:p2", "w1:t1");
    let task = pane_in("w1:p4", "w1:t3");

    client.save_pane(&lead).unwrap();
    client.save_pane(&task).unwrap();

    assert_eq!(client.load_pane(&lead.tab_id).unwrap(), Some(lead));
    assert_eq!(client.load_pane(&task.tab_id).unwrap(), Some(task));
}

/// Answer one request on `socket` with `result`, as Herdr does, and return the request.
fn answer_once(
    socket: &std::path::Path,
    result: serde_json::Value,
) -> std::thread::JoinHandle<serde_json::Value> {
    let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
    std::thread::spawn(move || {
        let (connection, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(connection.try_clone().unwrap());
        let request: serde_json::Value =
            serde_json::from_slice(&read_line(&mut reader, "test").unwrap()).unwrap();
        let response = serde_json::json!({"id": request["id"], "result": result});
        writeln!(&connection, "{response}").unwrap();
        request
    })
}

#[test_case("w1:t1", true; "a pane still in its tab is listed there")]
#[test_case("w1:t3", false; "a pane moved to another tab is not listed in its first tab")]
fn a_review_pane_is_listed_in_the_tab_herdr_reports(current_tab: &str, listed: bool) {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("socket");
    let client = HerdrClient::new(
        socket.clone(),
        "plugin".to_owned(),
        directory.path().join("state"),
    );
    let pane = PluginPane {
        pane_id: PaneId("w1:p2".to_owned()),
        tab_id: TabId("w1:t1".to_owned()),
        workspace_id: WorkspaceId("w1".to_owned()),
        entrypoint_id: EntrypointId("review".to_owned()),
    };
    client.save_pane(&pane).unwrap();
    let herdr = answer_once(
        &socket,
        serde_json::json!({"pane": {
            "pane_id": "w1:p2", "tab_id": current_tab, "workspace_id": "w1",
        }}),
    );

    let panes = HerdrReader::list_plugin_panes(&client, &pane.tab_id).unwrap();

    assert_eq!(herdr.join().unwrap()["params"]["pane_id"], "w1:p2");
    let expected = if listed {
        vec![pane.clone()]
    } else {
        Vec::new()
    };
    assert_eq!(panes, expected);
    assert_eq!(
        client.load_pane(&pane.tab_id).unwrap(),
        listed.then(|| pane.clone())
    );
    // A moved pane is recorded in the tab it is in now.
    let current = TabId(current_tab.to_owned());
    assert_eq!(
        client.load_pane(&current).unwrap(),
        Some(PluginPane {
            tab_id: current.clone(),
            ..pane
        })
    );
}

#[test]
fn a_temporary_record_is_not_taken_for_a_record_of_earlier_versions() {
    let directory = tempfile::tempdir().unwrap();
    let client = HerdrClient::new(
        directory.path().join("socket"),
        "plugin".to_owned(),
        directory.path().join("state"),
    );
    let pane = PluginPane {
        pane_id: PaneId("w1:p2".to_owned()),
        tab_id: TabId("w1:t1".to_owned()),
        workspace_id: WorkspaceId("w1".to_owned()),
        entrypoint_id: EntrypointId("review".to_owned()),
    };
    // What `save_pane` writes before its rename.
    let temporary = client.pane_path(&pane.tab_id).with_extension("tmp-1");
    std::fs::create_dir_all(temporary.parent().unwrap()).unwrap();
    std::fs::write(&temporary, serde_json::to_vec(&pane).unwrap()).unwrap();

    assert_eq!(client.tab_pane(&pane.tab_id).unwrap(), None);
    assert!(temporary.exists());
}

#[test]
fn a_record_keyed_by_workspace_moves_to_its_tab() {
    let directory = tempfile::tempdir().unwrap();
    let client = HerdrClient::new(
        directory.path().join("socket"),
        "plugin".to_owned(),
        directory.path().join("state"),
    );
    let pane = PluginPane {
        pane_id: PaneId("w1:p2".to_owned()),
        tab_id: TabId("w1:t1".to_owned()),
        workspace_id: WorkspaceId("w1".to_owned()),
        entrypoint_id: EntrypointId("review".to_owned()),
    };
    // Earlier versions named the record after the hex of the workspace ID: `w1` is `7731`.
    let old = directory.path().join("state/panes/7731.json");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, serde_json::to_vec(&pane).unwrap()).unwrap();

    assert_eq!(client.tab_pane(&pane.tab_id).unwrap(), Some(pane.clone()));
    assert!(!old.exists());
    assert_eq!(client.load_pane(&pane.tab_id).unwrap(), Some(pane));
}

#[test]
fn pane_record_read_errors_are_not_treated_as_missing() {
    let directory = tempfile::tempdir().unwrap();
    let client = HerdrClient::new(
        directory.path().join("socket"),
        "plugin".to_owned(),
        directory.path().join("state"),
    );
    let tab = TabId("tab".to_owned());
    std::fs::create_dir_all(client.pane_path(&tab)).unwrap();
    assert!(matches!(client.load_pane(&tab), Err(Error::Io { .. })));
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
