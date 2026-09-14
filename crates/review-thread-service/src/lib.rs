//! One serial owner for UI posts, MCP replies, persistence, and idle agent wakeups.

mod access;
mod notification;
mod prompt;
mod state;
mod wakeup;

use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use herdr_client::client::HerdrClient;
use herdr_client::protocol::{AgentTarget, HerdrEvent, PaneId};
use review_mcp_config::{Client, ProjectConfig};
use review_store::ReviewStore;
use review_threads::ThreadCommand;

use state::State;

/// UI commands accepted by the conversation owner.
#[derive(Debug)]
pub enum Command {
    Thread(ThreadCommand),
    Focus(PaneId),
    Observe(HerdrEvent),
}

enum Input {
    Ui(Command),
    Mcp(review_mcp::Request),
    Stop,
}

/// Conversation updates emitted after the authoritative state changes.
pub enum Event {
    Loaded(ui_events::ReviewThreadsLoaded),
    Posted(ui_events::ThreadPostFinished),
    Configured,
    NotificationDeferred,
    Error(String),
}

/// A reviewer-owned worker; dropping it closes its HTTP listener.
pub struct Worker {
    sender: Sender<Input>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Worker").finish_non_exhaustive()
    }
}

impl Worker {
    pub fn start(
        store: ReviewStore,
        client: HerdrClient,
        target: AgentTarget,
        connection: Result<ProjectConfig, String>,
        publish: impl Fn(Event) + Send + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let requests = sender.clone();
        let thread = thread::spawn(move || {
            let server = connection.and_then(|connection| {
                let server = review_mcp::Server::start(connection.endpoint(), move |request| {
                    requests
                        .send(Input::Mcp(request))
                        .map_err(|_| "The reviewer is closed".into())
                })?;
                Self::configure(&connection, &publish);
                Ok(server)
            });
            let available = server.is_ok();
            if let Err(error) = &server {
                publish(Event::Error(error.clone()));
            }
            State::new(store, client, target, available, Box::new(publish)).run(&receiver);
            drop(server);
        });
        Self {
            sender,
            thread: Some(thread),
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.sender.send(Input::Ui(command));
    }

    fn configure(connection: &ProjectConfig, publish: &impl Fn(Event)) {
        let mut changed = false;
        for client in Client::ALL {
            match connection.install(client) {
                Ok(added) => changed |= added,
                Err(error) => publish(Event::Error(error)),
            }
        }
        if changed {
            publish(Event::Configured);
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.sender.send(Input::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
