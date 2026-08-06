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
#[path = "worker.tests.rs"]
mod tests;
