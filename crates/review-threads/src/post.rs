use review_guide::DiffRangeAnchor;

use crate::{Message, MessageId, ThreadId, ThreadSource};
use std::sync::Arc;

/// One explicit publication, shared by the UI and the serial conversation owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Post {
    pub(super) thread_id: ThreadId,
    pub(super) message: Message,
    pub(super) source: Option<Arc<ThreadSource>>,
}

impl Post {
    pub fn start(anchor: DiffRangeAnchor, excerpt: String, text: String) -> Self {
        Self {
            thread_id: ThreadId(uuid::Uuid::new_v4().to_string()),
            message: Message::reviewer(text),
            source: Some(Arc::new(ThreadSource { anchor, excerpt })),
        }
    }

    pub fn reply(thread_id: ThreadId, text: String) -> Self {
        Self {
            thread_id,
            message: Message::reviewer(text),
            source: None,
        }
    }

    pub fn agent_reply(thread_id: ThreadId, id: MessageId, text: String) -> Self {
        Self {
            thread_id,
            message: Message::agent(id, text),
            source: None,
        }
    }

    /// Reply to a fetched snapshot without consuming comments posted afterward.
    pub fn answer(
        thread_id: ThreadId,
        id: MessageId,
        text: String,
        in_reply_to: MessageId,
    ) -> Self {
        let mut post = Self::agent_reply(thread_id, id, text);
        post.message.in_reply_to = Some(in_reply_to);
        post
    }

    pub fn message(&self) -> &Message {
        &self.message
    }

    pub fn thread_id(&self) -> &ThreadId {
        &self.thread_id
    }
}
