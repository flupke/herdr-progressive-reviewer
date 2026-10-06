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
fn a_stopped_command_channel_is_reported() {
    let (worker, commands, _events) = disconnected_worker();

    drop(commands);
    assert!(worker.open_document("source.rs".into()).is_err());
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
fn worker_waits_for_an_open_document_before_starting_rust_analyzer() {
    let directory = tempfile::tempdir().unwrap();
    let worker = Worker::start(directory.path().to_owned());
    let events = worker.event_receiver();

    // Dropping the worker ends its thread, which reports all it would before it ends.
    drop(worker);

    assert_eq!(
        events.try_recv(),
        Err(crossbeam_channel::TryRecvError::Disconnected)
    );
}

#[test]
fn rust_analyzer_finds_definitions_and_type_definitions() {
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
        "struct Answer;\nfn answer() -> Answer { Answer }\npub fn use_it() { let value = answer(); }\n",
    )
    .unwrap();
    let timeout = Duration::from_secs(30);
    let worker = Worker::start(directory.path().to_owned());
    worker.open_document(source.clone()).unwrap();
    assert!(matches!(
        worker.events.recv_timeout(timeout),
        Ok(Event::Initializing(_))
    ));
    let ready = worker.events.recv_timeout(timeout);
    assert!(matches!(ready, Ok(Event::Ready(_))), "{ready:?}");
    let text = "pub fn use_it() { let value = answer(); }";
    for (operation, symbol, target_line) in [
        (Operation::Definition, "answer", 1),
        (Operation::TypeDefinition, "value", 0),
    ] {
        let query = Query {
            toast_id: toasts::ToastId::generate(),
            path: source.clone(),
            line: 2,
            byte_column: text.find(symbol).unwrap(),
            expected_line: text.to_owned(),
            snapshot_id: "test".to_owned(),
        };
        let event = locations_once_indexed(&worker, operation, &query);
        let Event::Locations {
            operation: returned_operation,
            locations,
            ..
        } = event
        else {
            panic!("rust-analyzer did not return locations: {event:?}");
        };
        assert_eq!(returned_operation, operation);
        assert!(
            locations
                .iter()
                .any(|location| location.path == source && location.line == target_line),
            "{locations:?}"
        );
    }
}

/// Asks `operation` at `query` until the language server finds locations: rust-analyzer finds
/// none while it still indexes the crate after it became ready. Each attempt waits for the
/// answer to the one before.
fn locations_once_indexed(worker: &Worker, operation: Operation, query: &Query) -> Event {
    loop {
        worker
            .request(
                operation,
                Query {
                    toast_id: toasts::ToastId::generate(),
                    ..query.clone()
                },
            )
            .unwrap();
        let event = worker.events.recv().unwrap();
        let none = matches!(&event, Event::Locations { locations, .. } if locations.is_empty());
        if !none {
            return event;
        }
    }
}
