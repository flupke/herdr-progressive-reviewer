use std::fs;
use std::thread;
use std::time::Duration;

use crate::api::{Event, Operation, Query};
use crossbeam_channel::{Receiver, Sender, unbounded};

use super::Worker;

fn disconnected_worker() -> (Worker, Receiver<crate::api::Command>, Sender<Event>) {
    let (commands, received_commands) = unbounded();
    let (sent_events, events) = unbounded();
    (
        Worker {
            commands,
            events,
            handle: None,
        },
        received_commands,
        sent_events,
    )
}

fn query() -> Query {
    Query {
        toast_id: toasts::ToastId::generate(),
        path: "source.rs".into(),
        line: 1,
        byte_column: 2,
        expected_line: "line".to_owned(),
        snapshot_id: "snapshot".to_owned(),
    }
}

#[test]
fn public_commands_are_forwarded_to_the_server_channel() {
    let (worker, commands, _events) = disconnected_worker();
    worker.initialize().unwrap();
    assert_eq!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        crate::api::Command::Initialize
    );

    worker.open_document("source.rs".into()).unwrap();
    assert_eq!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        crate::api::Command::OpenDocument("source.rs".into())
    );

    let expected_query = query();
    worker
        .request(Operation::Hover, expected_query.clone())
        .unwrap();
    assert_eq!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        crate::api::Command::Request {
            operation: Operation::Hover,
            query: expected_query,
        }
    );

    worker.restart().unwrap();
    assert_eq!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap(),
        crate::api::Command::Restart
    );
}

#[test]
fn events_and_stopped_command_channels_are_reported() {
    let (worker, commands, events) = disconnected_worker();
    events.send(Event::Ready).unwrap();
    assert_eq!(worker.try_recv(), Some(Event::Ready));
    assert_eq!(worker.try_recv(), None);

    drop(commands);
    assert_eq!(worker.initialize().unwrap_err(), "LSP worker stopped");
}

#[test]
fn dropping_a_worker_sends_shutdown_and_joins_its_thread() {
    let (commands, received_commands) = unbounded();
    let (_sent_events, events) = unbounded();
    let (result_sender, result_receiver) = unbounded();
    let handle = thread::spawn(move || {
        let shutdown = received_commands.recv().ok();
        result_sender
            .send(shutdown == Some(crate::api::Command::Shutdown))
            .unwrap();
    });
    let worker = Worker {
        commands,
        events,
        handle: Some(handle),
    };

    drop(worker);

    assert_eq!(
        result_receiver.recv_timeout(Duration::from_secs(1)),
        Ok(true)
    );
}

#[test]
#[ignore = "requires rust-analyzer on PATH"]
fn rust_analyzer_finds_a_definition() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        "[package]\nname = \"lsp-smoke\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    let source = directory.path().join("src/lib.rs");
    fs::write(
        &source,
        "fn answer() -> u32 { 42 }\npub fn use_it() { answer(); }\n",
    )
    .unwrap();
    let timeout = Duration::from_secs(30);
    let worker = Worker::start(directory.path().to_owned());
    worker.initialize().unwrap();
    assert!(matches!(
        worker.events.recv_timeout(timeout),
        Ok(Event::Initializing)
    ));
    assert!(matches!(
        worker.events.recv_timeout(timeout),
        Ok(Event::Ready)
    ));
    worker.open_document(source.clone()).unwrap();
    thread::sleep(Duration::from_millis(500));
    worker
        .request(
            Operation::Definition,
            Query {
                toast_id: toasts::ToastId::generate(),
                path: source.clone(),
                line: 1,
                byte_column: 20,
                expected_line: "pub fn use_it() { answer(); }".to_owned(),
                snapshot_id: "test".to_owned(),
            },
        )
        .unwrap();
    let event = worker.events.recv_timeout(timeout).unwrap();
    let Event::Locations { locations, .. } = event else {
        panic!("rust-analyzer did not return locations: {event:?}");
    };
    assert!(
        locations
            .iter()
            .any(|location| location.path == source && location.line == 0),
        "{locations:?}"
    );
}
