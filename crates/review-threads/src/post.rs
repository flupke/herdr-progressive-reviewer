use review_guide::DiffRangeAnchor;

use crate::{Message, MessageId, ThreadId};

/// One explicit publication, shared by the UI and the serial conversation owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Post {
    pub(super) thread_id: ThreadId,
    pub(super) message: Message,
    pub(super) source: Option<Box<(DiffRangeAnchor, String)>>,
}

impl Post {
    pub fn start(anchor: DiffRangeAnchor, excerpt: String, text: String) -> Self {
        Self {
            thread_id: ThreadId(uuid::Uuid::new_v4().to_string()),
            message: Message::reviewer(text),
            source: Some(Box::new((anchor, excerpt))),
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

    pub fn message(&self) -> &Message {
        &self.message
    }

    pub fn thread_id(&self) -> &ThreadId {
        &self.thread_id
    }
}
