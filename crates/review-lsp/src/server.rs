use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, after, never, select};

use crate::api::{Command, Event};
use crate::session::Session;

pub(super) struct Server {
    root: PathBuf,
    events: Sender<Event>,
    session: Option<Session>,
    pending: VecDeque<Command>,
    stopping: bool,
}

impl Server {
    pub(super) fn new(root: PathBuf, events: Sender<Event>) -> Self {
        Self {
            root,
            events,
            session: None,
            pending: VecDeque::new(),
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
                        if !self.command(command) {
                            return;
                        }
                    }
                    Err(_) => return,
                },
                recv(inbound) -> message => match message {
                    Ok(message) => self.message(message),
                    Err(_) => self.fail_session("rust-analyzer stopped"),
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

    fn command(&mut self, command: Command) -> bool {
        if command == Command::Shutdown {
            self.stopping = true;
            self.pending.clear();
            if let Some(session) = &mut self.session {
                if session.begin_shutdown(Instant::now()).is_err() {
                    return false;
                }
                return true;
            }
            return false;
        }
        if self.session.is_none() {
            let _ = self.events.send(Event::Initializing);
            match Session::start(&self.root, Instant::now()) {
                Ok(session) => self.session = Some(session),
                Err(message) => {
                    let request = match &command {
                        Command::Request { query, .. } => {
                            (Some(query.toast_id), Some(query.snapshot_id.clone()))
                        }
                        _ => (None, None),
                    };
                    self.fail(request.0, request.1, message);
                    return true;
                }
            }
        }
        if command != Command::Initialize {
            self.pending.push_back(command);
        }
        true
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
            Ok(Some(event)) => {
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
                Command::Initialize | Command::Shutdown => Ok(()),
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
        let request = self
            .session
            .as_ref()
            .and_then(Session::active_query)
            .map_or((None, None), |query| {
                (Some(query.toast_id), Some(query.snapshot_id.clone()))
            });
        self.session = None;
        self.fail(request.0, request.1, message.to_owned());
        while let Some(command) = self.pending.pop_front() {
            if let Command::Request { query, .. } = command {
                self.fail(
                    Some(query.toast_id),
                    Some(query.snapshot_id),
                    message.to_owned(),
                );
            }
        }
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
