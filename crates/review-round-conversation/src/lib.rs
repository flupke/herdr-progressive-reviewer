//! The read model of an Explore round's conversation: the review thread attached to the round,
//! as plain data that a page can draw and a serde message can carry.

use review_threads::{
    AskedUnder, Author, Message, MessageId, ReviewThread, ReviewThreads, ThreadId, WakeupFailure,
};
use serde::Serialize;

/// The conversation of one Explore round, as the reviewer reads it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RoundConversation {
    /// The round, by its instance.
    pub round: String,
    /// The round's thread, once a first message started it.
    pub thread: Option<ThreadId>,
    /// The messages in posting order.
    pub messages: Vec<ConversationMessage>,
    /// The agent's replies the reviewer has not read.
    pub unread: usize,
    /// Whether Retry would wake the agent again for the reviewer's waiting messages. It holds
    /// for any waiting message, as the pane's Retry does; a page that offers Retry only on a
    /// message that was not delivered reads each message's `delivery` instead.
    pub can_retry: bool,
    /// The position to mark read through, once the reviewer saw these messages.
    pub read_through: u64,
}

/// One message of a round conversation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConversationMessage {
    pub id: MessageId,
    pub author: Author,
    pub text: String,
    /// When the review received it, in milliseconds since the Unix epoch.
    pub posted_at_ms: Option<u64>,
    /// Where in the round the reviewer wrote it.
    pub asked_under: Option<AskedUnder>,
    /// The passage of the round the reviewer quoted.
    pub quote: Option<String>,
    /// What became of a reviewer's message; an agent's reply has none.
    pub delivery: Option<Delivery>,
    /// Whether the reviewer has not read this reply of the agent.
    pub unread: bool,
}

/// What became of a message the reviewer wrote.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, ts_rs::TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Delivery {
    /// The agent has not replied yet: the wakeup is on its way, or the agent is working on it.
    Waiting,
    /// The wakeup that covered it did not reach the agent; Retry sends another. The reviewer
    /// keeps no record of it: after a restart the message reads as waiting, and Retry still
    /// applies.
    NotDelivered { error: String },
    /// An agent reply took it up.
    Answered,
}

impl RoundConversation {
    /// The conversation of `round` in `threads`, where the latest wakeup for the review's
    /// pending comments failed with `failure`, if it did: the failure of the thread worker's
    /// latest `Event::Wakeup` for the review, which is `None` again once another wakeup is on
    /// its way.
    pub fn new(threads: &ReviewThreads, round: &str, failure: Option<&WakeupFailure>) -> Self {
        let thread = threads.round_conversation(round);
        Self {
            round: round.to_owned(),
            thread: thread.map(|thread| thread.id.clone()),
            messages: thread
                .map(|thread| {
                    thread
                        .messages
                        .iter()
                        .map(|message| ConversationMessage::new(thread, message, failure))
                        .collect()
                })
                .unwrap_or_default(),
            unread: thread.map_or(0, |thread| {
                thread
                    .messages
                    .iter()
                    .filter(|message| thread.has_unread_reply(&message.id))
                    .count()
            }),
            can_retry: thread.is_some_and(|thread| threads.retry(&thread.id).is_ok()),
            read_through: threads.sequence(),
        }
    }
}

impl ConversationMessage {
    fn new(thread: &ReviewThread, message: &Message, failure: Option<&WakeupFailure>) -> Self {
        let delivery = (message.author == Author::Reviewer).then(|| {
            if thread.is_answered(message) {
                Delivery::Answered
            } else {
                match failure {
                    Some(failure) if message.sequence() <= failure.through => {
                        Delivery::NotDelivered {
                            error: failure.error.clone(),
                        }
                    }
                    _ => Delivery::Waiting,
                }
            }
        });
        Self {
            id: message.id.clone(),
            author: message.author,
            text: message.text.clone(),
            posted_at_ms: message.posted_at_ms,
            asked_under: message.asked_under.clone(),
            quote: message.quote.clone(),
            delivery,
            unread: thread.has_unread_reply(&message.id),
        }
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
