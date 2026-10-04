//! A fork's `stream-json` output: the tokens of its own requests, whether its submit has its
//! answer, and whether its turn ended.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use agent_fork::{Exit, ForkOutput};
use review_run_ahead::{ForkEnd, TokenUsage};
use serde_json::Value;

use crate::guard::SUBMITS;

/// Reads a fork's events as they come, then reports its end with what they told.
pub(crate) struct StreamTally {
    /// The usage of each assistant message, by message ID: a message comes in one event per
    /// content block, each with the message's usage.
    messages: HashMap<String, TokenUsage>,
    /// The fork's calls to a submit tool, by tool use ID.
    submits: HashSet<String>,
    /// Set once a submit's answer reached the fork, or the fork ended.
    answered: Arc<Signal>,
    /// Whether the turn's result came.
    finished: bool,
    ended: Box<dyn FnOnce(ForkEnd) + Send>,
}

/// A flag that threads wait for.
#[derive(Debug, Default)]
pub(crate) struct Signal {
    set: Mutex<bool>,
    changed: Condvar,
}

impl Signal {
    fn set(&self) {
        *self.set.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.changed.notify_all();
    }

    /// Waits at most `limit` for the flag; whether it was set.
    pub(crate) fn wait(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        let mut set = self.set.lock().unwrap_or_else(PoisonError::into_inner);
        while !*set {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            set = self
                .changed
                .wait_timeout(set, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

impl StreamTally {
    pub(crate) fn new(ended: Box<dyn FnOnce(ForkEnd) + Send>) -> Self {
        Self {
            messages: HashMap::new(),
            submits: HashSet::new(),
            answered: Arc::default(),
            finished: false,
            ended,
        }
    }

    /// Set once the answer to one of the fork's submits reached it, or once the fork ended:
    /// its transcript then holds that answer.
    pub(crate) fn submit_answered(&self) -> Arc<Signal> {
        Arc::clone(&self.answered)
    }

    /// The content blocks of `event`'s message.
    fn blocks(event: &Value) -> impl Iterator<Item = &Value> {
        event
            .pointer("/message/content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
    }

    fn usage(&self) -> TokenUsage {
        let mut total = TokenUsage::default();
        for usage in self.messages.values() {
            total += *usage;
        }
        total
    }
}

impl ForkOutput for StreamTally {
    fn line(&mut self, line: &str) {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        match event.get("type").and_then(serde_json::Value::as_str) {
            Some("assistant") => {
                let submits = Self::blocks(&event)
                    .filter(|block| {
                        block.get("type").and_then(Value::as_str) == Some("tool_use")
                            && block
                                .get("name")
                                .and_then(Value::as_str)
                                .is_some_and(|name| SUBMITS.contains(&name))
                    })
                    .filter_map(|block| block.get("id").and_then(Value::as_str));
                self.submits.extend(submits.map(str::to_owned));
                let id = event
                    .pointer("/message/id")
                    .and_then(serde_json::Value::as_str);
                let usage = event.pointer("/message/usage");
                if let (Some(id), Some(usage)) = (id, usage) {
                    let count = |name: &str| {
                        usage
                            .get(name)
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0)
                    };
                    self.messages.insert(
                        id.to_owned(),
                        TokenUsage {
                            input: count("input_tokens"),
                            cache_creation: count("cache_creation_input_tokens"),
                            cache_read: count("cache_read_input_tokens"),
                            output: count("output_tokens"),
                        },
                    );
                }
            }
            Some("user") => {
                let answered = Self::blocks(&event).any(|block| {
                    block.get("type").and_then(Value::as_str) == Some("tool_result")
                        && block
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .is_some_and(|id| self.submits.contains(id))
                });
                if answered {
                    self.answered.set();
                }
            }
            Some("result") => self.finished = true,
            _ => {}
        }
    }

    fn ended(self: Box<Self>, exit: Exit) {
        self.answered.set();
        let end = ForkEnd {
            exit: exit.to_string(),
            usage: self.usage(),
            finished: self.finished,
        };
        (self.ended)(end);
    }
}

#[cfg(test)]
#[path = "stream.tests.rs"]
mod tests;
