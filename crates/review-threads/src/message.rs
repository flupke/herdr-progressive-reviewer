use serde::{Deserialize, Serialize};

/// A stable message identity, also used to make posting retries idempotent.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MessageId(String);

impl MessageId {
    /// The identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validate an agent-supplied identity before accepting a reply.
    pub fn parse(value: &str) -> Result<Self, String> {
        let id = uuid::Uuid::parse_str(value).map_err(|_| "message_id must be a UUID")?;
        Ok(Self(id.to_string()))
    }
}

/// The author role of a posted thread message.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ts_rs::TS)]
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
    /// In a round conversation, where in the round the reviewer wrote this message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked_under: Option<crate::AskedUnder>,
    /// In a round conversation, the passage of the round the reviewer's message quotes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    /// When the review received the message, in milliseconds since the Unix epoch; messages
    /// of earlier builds have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posted_at_ms: Option<u64>,
    pub(super) sequence: u64,
}

impl Message {
    pub(super) fn reviewer(text: String) -> Self {
        Self {
            id: MessageId(uuid::Uuid::new_v4().to_string()),
            author: Author::Reviewer,
            text,
            in_reply_to: None,
            asked_under: None,
            quote: None,
            posted_at_ms: None,
            sequence: 0,
        }
    }

    pub(super) fn agent(id: MessageId, text: String) -> Self {
        Self {
            id,
            author: Author::Agent,
            text,
            in_reply_to: None,
            asked_under: None,
            quote: None,
            posted_at_ms: None,
            sequence: 0,
        }
    }

    /// The message's position among every message of its review, in posting order.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Record that the review receives the message now, by the reviewer's clock.
    pub(super) fn stamp_posting(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        self.posted_at_ms = Some(now);
    }

    /// Whether `other` posts the same message; the review stamps the time of posting.
    pub(super) fn same_content(&self, other: &Self) -> bool {
        self.id == other.id
            && self.author == other.author
            && self.text == other.text
            && self.in_reply_to == other.in_reply_to
            && self.asked_under == other.asked_under
            && self.quote == other.quote
    }
}
