//! One serial owner for UI posts, MCP replies, persistence, and agent notifications.

mod access;
mod agent_identity;
mod delivery;
mod notification;
mod pinned_agent;
mod round_prompt;
mod state;
mod wakeup;

use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use herdr_client::protocol::{AgentPort, AgentTarget, HerdrEvent};
use review_mcp::Endpoint;
use review_store::ReviewStore;
use review_threads::{MessageId, ThreadCommand, WakeupFailure};
use review_types::ReviewUnit;

pub use delivery::{
    DispatchObserver, Drained, PromptCancellation, PromptError, PromptHold, PromptReceipt,
    PromptSender,
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
    /// Answers once every earlier input is handled: each of their prompts whose agent is known
    /// is with the courier.
    #[cfg(any(test, feature = "flush"))]
    Flush(Sender<()>),
    /// The outcome of the comment notification sent with access `token` for the comments
    /// through sequence `through`.
    Notified {
        token: String,
        through: u64,
        result: Result<(), PromptError>,
    },
    /// Record each comments wakeup from now on in a vision session's turn log.
    LogTurns(vision_turns::TurnLog),
    Stop,
}

/// Conversation updates emitted after the authoritative state changes.
pub enum Event {
    Loaded(ui_events::ReviewThreadsLoaded),
    Posted(ui_events::ThreadPostFinished),
    /// The reviewer posted a new message in the conversation of an Explore round.
    RoundMessage(RoundMessage),
    /// What became of the latest wakeup for the pending comments of `review_unit`: `None`
    /// once one is on its way to the agent, or why one did not reach it.
    Wakeup {
        review_unit: ReviewUnit,
        failure: Option<WakeupFailure>,
    },
    Error(String),
}

/// A message the reviewer posted in the conversation of an Explore round.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundMessage {
    pub review_unit: ReviewUnit,
    /// The round, by its instance.
    pub round: String,
    pub message: MessageId,
}

/// A reviewer-owned worker; dropping it closes its HTTP listener.
pub struct Worker {
    sender: Sender<Input>,
    /// Holds the courier's prompts while a [`PromptHold`] lives.
    gate: std::sync::Arc<delivery::PromptGate>,
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
            gate: self.gate.clone(),
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
        let gate = std::sync::Arc::<delivery::PromptGate>::default();
        let courier_gate = gate.clone();
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
                courier_gate,
            )
            .run(&receiver);
            drop(server);
        });
        Self {
            sender,
            gate,
            thread: Some(thread),
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.sender.send(Input::Ui(command));
    }

    /// Records each comments wakeup in `turns`, a vision session's turn log, from the next one.
    pub fn log_turns(&self, turns: vision_turns::TurnLog) {
        let _ = self.sender.send(Input::LogTurns(turns));
    }

    /// Where another thread sends thread commands, as the pane's actions do: the Explore page
    /// writes the round's conversation through it.
    pub fn thread_commands(&self) -> ThreadCommands {
        ThreadCommands(self.sender.clone())
    }
}

/// Sends thread commands to the worker, from any thread.
#[derive(Clone)]
pub struct ThreadCommands(Sender<Input>);

impl ThreadCommands {
    pub fn send(&self, command: ThreadCommand) {
        let _ = self.0.send(Input::Ui(Command::Thread(command)));
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
