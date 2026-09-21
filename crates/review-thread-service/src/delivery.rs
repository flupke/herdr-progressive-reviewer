//! Reviewer prompts are submitted through Herdr by the conversation worker.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};

use herdr_client::client::HerdrClient;
use herdr_client::protocol::AgentPrompter;

use crate::{Input, PinnedAgent};

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

    fn deliver(&self, client: &HerdrClient) -> Result<bool, PromptError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(PromptError::Cancelled);
        }
        let Some(agent) = self.agent.current(client).map_err(PromptError::Delivery)? else {
            return Ok(false);
        };
        if self.cancelled.load(Ordering::Acquire) {
            return Err(PromptError::Cancelled);
        }
        if let Some(observer) = &self.observer {
            observer
                .before_attempt(&agent)
                .map_err(PromptError::Delivery)?;
        }
        // After the durable attempt marker, authoritative success wins a cancellation race.
        client
            .prompt_agent(&agent.pane_id, &self.text)
            .map_err(|error| PromptError::Unknown(error.to_string()))?;
        Ok(true)
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

    pub(super) fn poll(&mut self, client: &HerdrClient) {
        self.0.retain(|request| match request.deliver(client) {
            Ok(false) => true,
            result => {
                request.finish(result.map(|_| ()));
                false
            }
        });
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
