use std::path::PathBuf;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::api::{Command, Event, Operation, Query};
use crate::server::Server;

/// A running LSP worker.
pub struct Worker {
    commands: Sender<Command>,
    events: Receiver<Event>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// Start a worker for one repository root.
    pub fn start(root: PathBuf) -> Self {
        let (commands, command_receiver) = unbounded();
        let (event_sender, events) = unbounded();
        let handle = thread::spawn(move || {
            Server::new(root, event_sender).run(&command_receiver);
        });
        Self {
            commands,
            events,
            handle: Some(handle),
        }
    }

    /// Start rust-analyzer before the first request.
    pub fn initialize(&self) -> Result<(), String> {
        self.send(Command::Initialize)
    }

    /// Tell rust-analyzer about one open document.
    pub fn open_document(&self, path: PathBuf) -> Result<(), String> {
        self.send(Command::OpenDocument(path))
    }

    /// Run one LSP request.
    pub fn request(&self, operation: Operation, query: Query) -> Result<(), String> {
        self.send(Command::Request { operation, query })
    }

    /// Return the next available event without waiting.
    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|_| "LSP worker stopped".to_owned())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
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
}
