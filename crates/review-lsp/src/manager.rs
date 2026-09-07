use std::collections::HashMap;
use std::path::PathBuf;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::api::{Command, Event};
use crate::language::Project;
use crate::server::Server;

pub(super) struct Manager {
    root: PathBuf,
    events: Sender<Event>,
    servers: HashMap<Project, ServerWorker>,
}

impl Manager {
    pub(super) fn new(root: PathBuf, events: Sender<Event>) -> Self {
        Self {
            root,
            events,
            servers: HashMap::new(),
        }
    }

    pub(super) fn run(&mut self, commands: &Receiver<Command>) {
        while let Ok(command) = commands.recv() {
            match command {
                Command::Shutdown => return,
                Command::Restart => {
                    for server in self.servers.values() {
                        let _ = server.commands.send(Command::Restart);
                    }
                }
                command => self.route(command),
            }
        }
    }

    fn route(&mut self, mut command: Command) {
        let path = match &mut command {
            Command::OpenDocument(path) => path,
            Command::Request { query, .. } => &mut query.path,
            Command::Restart | Command::Shutdown => return,
        };
        if path.is_relative() {
            *path = self.root.join(&path);
        }
        let Some(project) = Project::for_document(&self.root, path) else {
            self.fail_request(&command, "No language server supports this file");
            return;
        };
        if matches!(&command, Command::OpenDocument(path) if !path.is_file()) {
            return;
        }
        let server = self
            .servers
            .entry(project.clone())
            .or_insert_with(|| ServerWorker::start(project, self.events.clone()));
        if let Err(error) = server.commands.send(command) {
            self.fail_request(&error.0, "Language server worker stopped");
        }
    }

    fn fail_request(&self, command: &Command, message: &str) {
        if let Command::Request { query, .. } = command {
            let _ = self.events.send(Event::Failed {
                toast_id: Some(query.toast_id),
                snapshot_id: Some(query.snapshot_id.clone()),
                message: message.to_owned(),
            });
        }
    }
}

struct ServerWorker {
    commands: Sender<Command>,
    handle: Option<JoinHandle<()>>,
}

impl ServerWorker {
    fn start(project: Project, events: Sender<Event>) -> Self {
        let (commands, receiver) = unbounded();
        let handle = thread::spawn(move || {
            Server::new(project, events).run(&receiver);
        });
        Self {
            commands,
            handle: Some(handle),
        }
    }
}

impl Drop for ServerWorker {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
#[path = "manager.tests.rs"]
mod tests;
