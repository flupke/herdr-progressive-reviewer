use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, after, never, select};

use crate::api::{Command, Event, ServerStartup};
use crate::language::Project;
use crate::session::Session;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServerLoopControl {
    Continue,
    Stop,
}

pub(super) struct Server {
    project: Project,
    events: Sender<Event>,
    session: Option<Session>,
    startup: Option<ServerStartup>,
    pending: VecDeque<Command>,
    open_documents: BTreeSet<PathBuf>,
    stopping: bool,
}

impl Server {
    pub(super) fn new(project: Project, events: Sender<Event>) -> Self {
        Self {
            project,
            events,
            session: None,
            startup: None,
            pending: VecDeque::new(),
            open_documents: BTreeSet::new(),
            stopping: false,
        }
    }

    pub(super) fn run(&mut self, commands: &Receiver<Command>) {
        loop {
            let inbound = self
                .session
                .as_ref()
                .map_or_else(never, |session| session.inbound().clone());
            let deadline = self
                .session
                .as_ref()
                .and_then(Session::next_deadline)
                .map_or_else(never, |deadline| {
                    after(deadline.saturating_duration_since(Instant::now()))
                });
            select! {
                recv(commands) -> command => match command {
                    Ok(command) => {
                        if self.command(command) == ServerLoopControl::Stop {
                            return;
                        }
                    }
                    Err(_) => return,
                },
                recv(inbound) -> message => match message {
                    Ok(message) => self.message(message),
                    Err(_) => self.fail_session("language server stopped"),
                },
                recv(deadline) -> _ => self.handle_deadline(),
            }
            if self.session.as_ref().is_some_and(Session::is_stopped) {
                return;
            }
            if self.stopping && self.session.is_none() {
                return;
            }
            self.dispatch_pending_commands();
        }
    }

    fn command(&mut self, command: Command) -> ServerLoopControl {
        match command {
            Command::Shutdown => self.shutdown(),
            Command::Restart => {
                self.restart();
                ServerLoopControl::Continue
            }
            command => {
                self.queue(command);
                ServerLoopControl::Continue
            }
        }
    }

    fn shutdown(&mut self) -> ServerLoopControl {
        self.stopping = true;
        self.pending.clear();
        let Some(session) = &mut self.session else {
            return ServerLoopControl::Stop;
        };
        if session.begin_shutdown(Instant::now()).is_ok() {
            ServerLoopControl::Continue
        } else {
            ServerLoopControl::Stop
        }
    }

    fn queue(&mut self, command: Command) {
        match &command {
            Command::OpenDocument(path) => {
                self.open_documents.insert(path.clone());
            }
            Command::Request { query, .. } => {
                self.open_documents.insert(query.path.clone());
            }
            Command::Restart | Command::Shutdown => return,
        }
        self.pending.push_back(command);
        if self.session.is_none()
            && let Err(message) = self.start_session()
        {
            self.fail_session(&message);
        }
    }

    fn restart(&mut self) {
        let _ = self.fail_requests("language server restarted");
        self.pending = self
            .open_documents
            .iter()
            .cloned()
            .map(Command::OpenDocument)
            .collect();
        self.session = None;
        if self.open_documents.is_empty() {
            return;
        }
        if let Err(message) = self.start_session() {
            self.fail_session(&message);
        }
    }

    fn start_session(&mut self) -> Result<(), String> {
        let startup = ServerStartup {
            id: toasts::ToastId::generate(),
            name: self.project.server.name(),
        };
        self.startup = Some(startup);
        let _ = self.events.send(Event::Initializing(startup));
        self.session = Some(Session::start(&self.project, startup, Instant::now())?);
        Ok(())
    }

    fn fail_requests(&mut self, message: &str) -> bool {
        let mut failed_request = false;
        if let Some(startup) = self.startup.take() {
            self.fail(Some(startup.id), None, message.to_owned());
            failed_request = true;
        }
        if let Some(query) = self.session.as_ref().and_then(Session::active_query) {
            self.fail(
                Some(query.toast_id),
                Some(query.snapshot_id.clone()),
                message.to_owned(),
            );
            failed_request = true;
        }
        for command in &self.pending {
            if let Command::Request { query, .. } = command {
                self.fail(
                    Some(query.toast_id),
                    Some(query.snapshot_id.clone()),
                    message.to_owned(),
                );
                failed_request = true;
            }
        }
        failed_request
    }

    fn message(&mut self, message: crate::session::Inbound) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let result = session.handle(message, Instant::now());
        self.handle_session_result(result);
    }

    fn handle_session_result(&mut self, result: Result<Option<Event>, String>) {
        match result {
            Ok(Some(mut event)) => {
                if matches!(event, Event::Ready(_)) {
                    self.startup = None;
                }
                if let Event::Locations {
                    operation,
                    locations,
                    ..
                } = &mut event
                {
                    *locations =
                        operation.filter_locations(&self.project.root, std::mem::take(locations));
                }
                let _ = self.events.send(event);
            }
            Ok(None) => {}
            Err(message) => self.fail_session(&message),
        }
    }

    fn handle_deadline(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let result = session.handle_deadline(Instant::now());
        self.handle_session_result(result);
    }

    fn dispatch_pending_commands(&mut self) {
        while self.session.as_ref().is_some_and(Session::is_ready) {
            let Some(command) = self.pending.pop_front() else {
                return;
            };
            let request = match &command {
                Command::Request { query, .. } => {
                    (Some(query.toast_id), Some(query.snapshot_id.clone()))
                }
                _ => (None, None),
            };
            let result = match command {
                Command::OpenDocument(path) => self
                    .session
                    .as_mut()
                    .expect("session exists")
                    .open_document(&path),
                Command::Request { operation, query } => self
                    .session
                    .as_mut()
                    .expect("session exists")
                    .request(operation, query, Instant::now()),
                Command::Restart | Command::Shutdown => Ok(()),
            };
            if let Err(message) = result {
                self.fail(request.0, request.1, message);
            }
        }
    }

    fn fail_session(&mut self, message: &str) {
        if self.stopping {
            self.session = None;
            return;
        }
        if !self.fail_requests(message) && (self.session.is_some() || !self.pending.is_empty()) {
            self.fail(None, None, message.to_owned());
        }
        self.session = None;
        self.pending.clear();
    }

    fn fail(
        &self,
        toast_id: Option<toasts::ToastId>,
        snapshot_id: Option<String>,
        message: String,
    ) {
        let _ = self.events.send(Event::Failed {
            toast_id,
            snapshot_id,
            message,
        });
    }
}

#[cfg(test)]
#[path = "server.tests.rs"]
mod tests;
