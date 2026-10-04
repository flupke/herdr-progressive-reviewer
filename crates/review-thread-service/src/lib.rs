//! One serial owner for UI posts, MCP replies, persistence, and agent notifications.

mod access;
mod agent_identity;
mod delivery;
mod notification;
mod pinned_agent;
mod state;
mod wakeup;

use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use herdr_client::protocol::{AgentPort, AgentTarget, HerdrEvent};
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
    /// The outcome of the comment notification sent with access `token` for the comments
    /// through sequence `through`.
    Notified {
        token: String,
        through: u64,
        result: Result<(), PromptError>,
    },
    Stop,
}

/// Conversation updates emitted after the authoritative state changes.
pub enum Event {
    Loaded(ui_events::ReviewThreadsLoaded),
    Posted(ui_events::ThreadPostFinished),
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

    /// Serve MCP at `endpoint`. Explore operations go straight to `explore`, which
    /// belongs to the Explore session; this worker handles only review threads.
    pub fn start(
        store: ReviewStore,
        port: impl AgentPort + 'static,
        target: AgentTarget,
        endpoint: Result<Endpoint, String>,
        explore: impl Fn(review_mcp::Request) -> Result<(), String> + Send + Sync + 'static,
        publish: impl Fn(Event) + Send + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let requests = sender.clone();
        let inputs = sender.clone();
        let thread = thread::spawn(move || {
            let server = endpoint.and_then(|endpoint| {
                review_mcp::Server::start(endpoint, move |request| {
                    if request.operation.belongs_to_explore() {
                        return explore(request);
                    }
                    requests
                        .send(Input::Mcp(request))
                        .map_err(|_| "The reviewer is closed".into())
                })
            });
            let available = server.is_ok();
            if let Err(error) = &server {
                publish(Event::Error(error.clone()));
            }
            State::new(
                store,
                std::sync::Arc::new(port),
                target,
                available,
                Box::new(publish),
                inputs,
            )
            .run(&receiver);
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
