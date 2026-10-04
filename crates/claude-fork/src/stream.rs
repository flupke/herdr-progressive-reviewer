//! A fork's `stream-json` output: the tokens of its own requests, and whether its turn ended.

use std::collections::HashMap;

use agent_fork::{Exit, ForkOutput};
use review_run_ahead::{ForkEnd, TokenUsage};

/// Reads a fork's events as they come, then reports its end with what they told.
pub(crate) struct StreamTally {
    /// The usage of each assistant message, by message ID: a message comes in one event per
    /// content block, each with the message's usage.
    messages: HashMap<String, TokenUsage>,
    /// Whether the turn's result came.
    finished: bool,
    ended: Box<dyn FnOnce(ForkEnd) + Send>,
}

impl StreamTally {
    pub(crate) fn new(ended: Box<dyn FnOnce(ForkEnd) + Send>) -> Self {
        Self {
            messages: HashMap::new(),
            finished: false,
            ended,
        }
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
            Some("result") => self.finished = true,
            _ => {}
        }
    }

    fn ended(self: Box<Self>, exit: Exit) {
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
