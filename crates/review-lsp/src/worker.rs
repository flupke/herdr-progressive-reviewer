use std::path::PathBuf;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::api::{Command, Event, Operation, Query};
use crate::manager::Manager;

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
            Manager::new(root, event_sender).run(&command_receiver);
        });
        Self {
            commands,
            events,
            handle: Some(handle),
        }
    }

    /// Open a supported document, starting its language server when necessary.
    pub fn open_document(&self, path: PathBuf) -> Result<(), String> {
        self.send(Command::OpenDocument(path))
    }

    /// Run one LSP request.
    pub fn request(&self, operation: Operation, query: Query) -> Result<(), String> {
        self.send(Command::Request { operation, query })
    }

    /// Restart active language servers and reopen known documents.
    pub fn restart(&self) -> Result<(), String> {
        self.send(Command::Restart)
    }

    /// Return the next available event without waiting.
    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Return a receiver that can forward worker events to an application channel.
    pub fn event_receiver(&self) -> Receiver<Event> {
        self.events.clone()
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
