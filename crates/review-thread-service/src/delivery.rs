//! Reviewer prompts are submitted through Herdr: the conversation worker decides what to send,
//! and its courier sends it.
use std::sync::{
    Arc, Condvar, Mutex, PoisonError,
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

/// Keeps the courier from sending while any [`PromptHold`] lives, as while run-ahead switches
/// the agent of a pane to another session: no prompt may reach the agent meanwhile.
#[derive(Default)]
pub(super) struct PromptGate {
    state: Mutex<GateState>,
    changed: Condvar,
}

#[derive(Default)]
struct GateState {
    /// The holds that live.
    holds: usize,
    /// Whether the courier is sending a prompt.
    sending: bool,
}

impl PromptGate {
    fn lock(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sends `delivery` once no hold lives; no hold begins while it is sent.
    fn send(&self, delivery: QueuedPrompt, port: &dyn AgentPort) {
        let mut state = self
            .changed
            .wait_while(self.lock(), |state| state.holds > 0)
            .unwrap_or_else(PoisonError::into_inner);
        state.sending = true;
        drop(state);
        let _sent = Sent(self);
        delivery.send(port);
    }
}

/// Marks the courier's prompt sent once dropped, even when sending it panicked.
struct Sent<'a>(&'a PromptGate);

impl Drop for Sent<'_> {
    fn drop(&mut self) {
        self.0.lock().sending = false;
        self.0.changed.notify_all();
    }
}

/// Holds every prompt the courier has not begun to send, the thread service's wakeups
/// included, until it is dropped. A prompt withdrawn meanwhile is not sent. A prompt the
/// courier was sending when the hold began may still be on its way: [`PromptHold::drained`]
/// waits for it.
#[must_use = "prompts are held only while the hold lives"]
pub struct PromptHold(Arc<PromptGate>);

impl PromptHold {
    fn new(gate: &Arc<PromptGate>) -> Self {
        gate.lock().holds += 1;
        Self(Arc::clone(gate))
    }

    /// What tells, on any thread, when the prompt the courier was sending as the hold began,
    /// if any, is sent.
    pub fn drained(&self) -> Drained {
        Drained(Arc::clone(&self.0))
    }
}

/// Tells when no prompt is on its way any more, while a [`PromptHold`] lives.
pub struct Drained(Arc<PromptGate>);

impl Drained {
    /// Waits until the courier sends no prompt.
    pub fn wait(self) {
        drop(
            self.0
                .changed
                .wait_while(self.0.lock(), |state| state.sending)
                .unwrap_or_else(PoisonError::into_inner),
        );
    }
}

impl Drop for PromptHold {
    fn drop(&mut self) {
        self.0.lock().holds -= 1;
        self.0.changed.notify_all();
    }
}

impl std::fmt::Debug for PromptHold {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PromptHold")
    }
}

/// Sends prompts on a thread of its own, one at a time and in order, so that the worker keeps
/// serving the reviewer and the agents while Herdr waits for an agent to start on a prompt.
pub(super) struct Courier(Sender<Parcel>);

impl Courier {
    pub(super) fn start(port: Arc<dyn AgentPort>, gate: Arc<PromptGate>) -> Self {
        let (sender, parcels) = mpsc::channel();
        // Not joined: a prompt in flight may wait for its agent for seconds, and its outcome
        // still reaches its owner after the worker stops.
        thread::spawn(move || {
            for parcel in parcels {
                match parcel {
                    Parcel::Prompt(delivery) => gate.send(*delivery, &*port),
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

#[derive(Clone)]
pub struct PromptSender {
    pub(super) sender: Sender<Input>,
    pub(super) gate: Arc<PromptGate>,
}

impl std::fmt::Debug for PromptSender {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PromptSender")
            .finish_non_exhaustive()
    }
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
    /// Holds every prompt not yet on its way to an agent, this sender's and the thread
    /// service's, until the hold is dropped.
    pub fn hold(&self) -> PromptHold {
        PromptHold::new(&self.gate)
    }

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
    fn a_held_courier_sends_nothing_until_the_hold_ends_and_skips_a_prompt_withdrawn_meanwhile() {
        let agents = herdr_client::memory::InMemoryAgents::default();
        let agent: Agent = serde_json::from_value(serde_json::json!({"pane_id":"pane", "tab_id":"tab", "workspace_id":"workspace", "agent_status":"idle"})).unwrap();
        agents.upsert_agent(agent.clone());
        let gate = Arc::<PromptGate>::default();
        let courier = Courier::start(Arc::new(agents.clone()), gate.clone());
        let hold = PromptHold::new(&gate);
        let (outcomes, outcome) = mpsc::channel();
        let withdrawn = Arc::new(AtomicBool::new(false));
        for (text, cancelled) in [("first", None), ("withdrawn", Some(withdrawn.clone()))] {
            let outcomes = outcomes.clone();
            courier.send(agent.clone(), text.into(), cancelled, move |result| {
                outcomes.send((text, result.is_ok())).unwrap();
            });
        }

        thread::sleep(std::time::Duration::from_millis(100));
        assert!(agents.prompts().is_empty(), "nothing is sent while held");
        withdrawn.store(true, Ordering::Release);
        drop(hold);
        courier.flush();

        assert_eq!(
            agents
                .prompts()
                .iter()
                .map(|prompt| prompt.text.as_str())
                .collect::<Vec<_>>(),
            ["first"]
        );
        assert_eq!(outcome.recv().unwrap(), ("first", true));
        assert_eq!(outcome.recv().unwrap(), ("withdrawn", false));
    }

    #[test]
    fn closed_queue_reports_definite_non_delivery_before_returning_a_receipt() {
        let (sender, receiver) = mpsc::channel();
        drop(receiver);
        let agent = serde_json::from_value(serde_json::json!({"pane_id":"pane", "tab_id":"tab", "workspace_id":"workspace", "agent_status":"idle"})).unwrap();
        let observer = Arc::new(OutcomeObserver(AtomicBool::new(false)));
        let (receipt, _guard) = PromptSender {
            sender,
            gate: Arc::default(),
        }
        .send_observed(
            PinnedAgent::new(agent),
            "Prepared prompt".into(),
            Some(observer.clone()),
        );
        assert!(observer.0.load(Ordering::Acquire));
        assert!(matches!(receipt.wait(), Err(PromptError::Delivery(_))));
    }
}
