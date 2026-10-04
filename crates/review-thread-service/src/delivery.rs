//! Reviewer prompts are submitted through Herdr: the conversation worker decides what to send,
//! and its courier sends it.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};
use std::thread;

use herdr_client::protocol::{Agent, AgentPort};

use crate::{Input, PinnedAgent};

/// Submit `text` to `agent` and wait until it starts on it. Any other failure may come after
/// Herdr wrote the text, so it leaves the outcome unknown.
fn prompt_agent(port: &dyn AgentPort, agent: &Agent, text: &str) -> Result<(), PromptError> {
    port.prompt_agent(&agent.pane_id, text)
        .map_err(|error| match error {
            herdr_client::Error::AgentNotStarted { .. } => PromptError::NotStarted,
            error => PromptError::Unknown(error.to_string()),
        })
}

/// What the courier calls with the outcome of one prompt, on its own thread.
type Outcome = Box<dyn FnOnce(Result<(), PromptError>) + Send>;

/// One prompt for the courier to send, and where its outcome goes.
struct QueuedPrompt {
    agent: Agent,
    text: String,
    /// Set when the owner withdraws the prompt; one still waiting behind another is not sent.
    cancelled: Option<Arc<AtomicBool>>,
    outcome: Outcome,
}

impl QueuedPrompt {
    fn send(self, port: &dyn AgentPort) {
        let withdrawn = self
            .cancelled
            .as_ref()
            .is_some_and(|cancelled| cancelled.load(Ordering::Acquire));
        let result = if withdrawn {
            Err(PromptError::Cancelled)
        } else {
            prompt_agent(port, &self.agent, &self.text)
        };
        (self.outcome)(result);
    }
}

enum Parcel {
    Prompt(Box<QueuedPrompt>),
    /// Answers once every earlier prompt is sent.
    #[cfg(test)]
    Flush(Sender<()>),
}

/// Sends prompts on a thread of its own, one at a time and in order, so that the worker keeps
/// serving the reviewer and the agents while Herdr waits for an agent to start on a prompt.
pub(super) struct Courier(Sender<Parcel>);

impl Courier {
    pub(super) fn start(port: Arc<dyn AgentPort>) -> Self {
        let (sender, parcels) = mpsc::channel();
        // Not joined: a prompt in flight may wait for its agent for seconds, and its outcome
        // still reaches its owner after the worker stops.
        thread::spawn(move || {
            for parcel in parcels {
                match parcel {
                    Parcel::Prompt(delivery) => delivery.send(&*port),
                    #[cfg(test)]
                    Parcel::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        Self(sender)
    }

    /// Send `text` to `agent` after the prompts sent before it, unless `cancelled` is set by
    /// then, and report the outcome.
    pub(super) fn send(
        &self,
        agent: Agent,
        text: String,
        cancelled: Option<Arc<AtomicBool>>,
        outcome: impl FnOnce(Result<(), PromptError>) + Send + 'static,
    ) {
        let parcel = Parcel::Prompt(Box::new(QueuedPrompt {
            agent,
            text,
            cancelled,
            outcome: Box::new(outcome),
        }));
        if let Err(mpsc::SendError(Parcel::Prompt(delivery))) = self.0.send(parcel) {
            (delivery.outcome)(Err(PromptError::Delivery(
                "The reviewer prompt courier stopped; nothing was sent".into(),
            )));
        }
    }

    /// Wait until every prompt sent so far has its outcome.
    #[cfg(test)]
    pub(super) fn flush(&self) {
        let (done, flushed) = mpsc::channel();
        self.0.send(Parcel::Flush(done)).unwrap();
        flushed.recv().unwrap();
    }
}

#[derive(Clone, Debug)]
pub struct PromptSender {
    pub(super) sender: Sender<Input>,
}

/// A queued request completes once, after delivery or a permanent failure.
pub struct PromptReceipt(Receiver<Result<(), PromptError>>);

#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    #[error("The reviewer prompt was cancelled")]
    Cancelled,
    #[error("{0}")]
    Delivery(String),
    #[error("The agent did not start on the prompt. Look at the agent's pane, then Retry.")]
    NotStarted,
    #[error("Delivery outcome unknown: {0}")]
    Unknown(String),
}

/// Optional durable boundary for a caller's intent before external delivery.
pub trait DispatchObserver: Send + Sync {
    fn before_attempt(&self, agent: &herdr_client::protocol::Agent) -> Result<(), String>;
    fn finished(&self, result: &Result<(), PromptError>) -> Result<(), String>;
}

impl PromptReceipt {
    pub fn wait(self) -> Result<(), PromptError> {
        self.0.recv().unwrap_or_else(|_| {
            Err(PromptError::Unknown(
                "The reviewer prompt dispatcher closed".into(),
            ))
        })
    }
}

/// Dropping the owner's guard removes a prompt that has not yet been sent.
#[derive(Debug)]
pub struct PromptCancellation(Arc<AtomicBool>);

impl Drop for PromptCancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl PromptSender {
    pub fn send(&self, agent: PinnedAgent, text: String) -> (PromptReceipt, PromptCancellation) {
        self.send_observed(agent, text, None)
    }

    pub fn send_observed(
        &self,
        agent: PinnedAgent,
        text: String,
        observer: Option<Arc<dyn DispatchObserver>>,
    ) -> (PromptReceipt, PromptCancellation) {
        let cancelled = Arc::new(AtomicBool::new(false));
        let (response, received) = mpsc::channel();
        if let Err(mpsc::SendError(Input::Prompt(request))) =
            self.sender.send(Input::Prompt(PromptRequest {
                agent,
                text,
                cancelled: cancelled.clone(),
                response,
                observer,
            }))
        {
            request.finish(Err(PromptError::Delivery(
                "The reviewer prompt dispatcher is unavailable; nothing was sent".into(),
            )));
        }
        (PromptReceipt(received), PromptCancellation(cancelled))
    }
}

pub(super) struct PromptRequest {
    agent: PinnedAgent,
    text: String,
    cancelled: Arc<AtomicBool>,
    response: Sender<Result<(), PromptError>>,
    observer: Option<Arc<dyn DispatchObserver>>,
}

impl PromptRequest {
    fn finish(&self, mut result: Result<(), PromptError>) {
        if let Some(observer) = &self.observer
            && let Err(error) = observer.finished(&result)
        {
            result = Err(PromptError::Unknown(error));
        }
        let _ = self.response.send(result);
    }

    /// Begin the attempt, and return the agent to send the prompt to; `None` while the
    /// pinned agent is not known yet.
    fn begin(&self, port: &dyn AgentPort) -> Result<Option<Agent>, PromptError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(PromptError::Cancelled);
        }
        let Some(agent) = self.agent.current(port).map_err(PromptError::Delivery)? else {
            return Ok(None);
        };
        if self.cancelled.load(Ordering::Acquire) {
            return Err(PromptError::Cancelled);
        }
        if let Some(observer) = &self.observer {
            observer
                .before_attempt(&agent)
                .map_err(PromptError::Delivery)?;
        }
        self.agent.seal_attempt().map_err(PromptError::Delivery)?;
        Ok(Some(agent))
    }
}

#[derive(Default)]
pub(super) struct PromptQueue(Vec<PromptRequest>);

impl PromptQueue {
    pub(super) fn push(&mut self, request: PromptRequest) {
        self.0.push(request);
    }

    pub(super) fn is_pending(&self) -> bool {
        !self.0.is_empty()
    }

    pub(super) fn poll(&mut self, port: &dyn AgentPort, courier: &Courier) {
        for request in std::mem::take(&mut self.0) {
            match request.begin(port) {
                // After the durable attempt marker, a prompt the courier sent wins a cancellation
                // race; one still waiting behind another prompt is withdrawn.
                Ok(Some(agent)) => {
                    let text = request.text.clone();
                    let cancelled = request.cancelled.clone();
                    courier.send(agent, text, Some(cancelled), move |result| {
                        request.finish(result);
                    });
                }
                Ok(None) => self.0.push(request),
                Err(error) => request.finish(Err(error)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OutcomeObserver(AtomicBool);

    impl DispatchObserver for OutcomeObserver {
        fn before_attempt(&self, _: &herdr_client::protocol::Agent) -> Result<(), String> {
            panic!("A closed queue must never start an external attempt");
        }

        fn finished(&self, result: &Result<(), PromptError>) -> Result<(), String> {
            assert!(matches!(result, Err(PromptError::Delivery(_))));
            self.0.store(true, Ordering::Release);
            Ok(())
        }
    }

    #[test]
    fn closed_queue_reports_definite_non_delivery_before_returning_a_receipt() {
        let (sender, receiver) = mpsc::channel();
        drop(receiver);
        let agent = serde_json::from_value(serde_json::json!({"pane_id":"pane", "tab_id":"tab", "workspace_id":"workspace", "agent_status":"idle"})).unwrap();
        let observer = Arc::new(OutcomeObserver(AtomicBool::new(false)));
        let (receipt, _guard) = PromptSender { sender }.send_observed(
            PinnedAgent::new(agent),
            "Prepared prompt".into(),
            Some(observer.clone()),
        );
        assert!(observer.0.load(Ordering::Acquire));
        assert!(matches!(receipt.wait(), Err(PromptError::Delivery(_))));
    }
}
