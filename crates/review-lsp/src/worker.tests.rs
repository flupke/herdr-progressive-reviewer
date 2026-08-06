use std::fs;
use std::thread;
use std::time::Duration;

use crate::api::{Event, Operation, Query};

use super::Worker;

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
