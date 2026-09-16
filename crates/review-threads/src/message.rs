use serde::{Deserialize, Serialize};

/// A stable message identity, also used to make posting retries idempotent.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MessageId(String);

impl MessageId {
    /// Validate an agent-supplied identity before accepting a reply.
    pub fn parse(value: &str) -> Result<Self, String> {
        let id = uuid::Uuid::parse_str(value).map_err(|_| "message_id must be a UUID")?;
        Ok(Self(id.to_string()))
    }
}

/// The author role of a posted thread message.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Author {
    Reviewer,
    Agent,
}

/// An immutable contribution to a thread's chronological conversation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Message {
    pub id: MessageId,
    pub author: Author,
    pub text: String,
    /// Last reviewer comment covered by this agent reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<MessageId>,
    pub(super) sequence: u64,
}

impl Message {
    pub(super) fn reviewer(text: String) -> Self {
        Self {
            id: MessageId(uuid::Uuid::new_v4().to_string()),
            author: Author::Reviewer,
            text,
            in_reply_to: None,
            sequence: 0,
        }
    }

    pub(super) fn agent(id: MessageId, text: String) -> Self {
        Self {
            id,
            author: Author::Agent,
            text,
            in_reply_to: None,
            sequence: 0,
        }
    }

    pub(super) fn same_content(&self, other: &Self) -> bool {
        self.id == other.id
            && self.author == other.author
            && self.text == other.text
            && self.in_reply_to == other.in_reply_to
    }
}
