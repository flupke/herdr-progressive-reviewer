//! One serial owner for UI posts, MCP replies, persistence, and idle agent wakeups.

mod access;
mod delivery;
mod notification;
mod pinned_agent;
mod prompt;
mod state;
mod wakeup;

use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use herdr_client::client::HerdrClient;
use herdr_client::protocol::{AgentTarget, HerdrEvent};
use review_mcp::Endpoint;
use review_store::ReviewStore;
use review_threads::ThreadCommand;

pub use delivery::{
    DispatchObserver, PromptCancellation, PromptError, PromptReceipt, PromptSender,
};
pub use pinned_agent::PinnedAgent;
use state::State;

/// UI commands accepted by the conversation owner.
#[derive(Debug)]
pub enum Command {
    Thread(ThreadCommand),
    /// The shared active-agent selection changed.
    ActiveAgentChanged,
    Observe(HerdrEvent),
}

enum Input {
    Ui(Command),
    Mcp(review_mcp::Request),
    Prompt(delivery::PromptRequest),
    Stop,
}

/// Conversation updates emitted after the authoritative state changes.
pub enum Event {
    /// Explore has its own in-memory decision owner; it never changes ordinary threads.
    Explore(review_mcp::Request),
    Loaded(ui_events::ReviewThreadsLoaded),
    Posted(ui_events::ThreadPostFinished),
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
    pub fn prompt_sender(&self) -> PromptSender {
        PromptSender {
            sender: self.sender.clone(),
        }
    }

    pub fn start(
        store: ReviewStore,
        client: HerdrClient,
        target: AgentTarget,
        endpoint: Result<Endpoint, String>,
        publish: impl Fn(Event) + Send + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let requests = sender.clone();
        let thread = thread::spawn(move || {
            let server = endpoint.and_then(|endpoint| {
                review_mcp::Server::start(endpoint, move |request| {
                    requests
                        .send(Input::Mcp(request))
                        .map_err(|_| "The reviewer is closed".into())
                })
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
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.sender.send(Input::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
