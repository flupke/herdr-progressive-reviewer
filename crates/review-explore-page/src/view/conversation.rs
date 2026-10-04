//! The round's conversation with the agent, as the chat shows it: the review thread attached to
//! the round, read through `review_round_conversation`, with each message's Markdown rendered and
//! the place in the round it was written at named as the rail names it.

use review_round_conversation::{ConversationMessage, Delivery, RoundConversation};
use review_threads::{AskedUnder, Author};
use serde::Serialize;
use ts_rs::TS;

use super::markdown;
use crate::conversation::ThreadsSnapshot;
use crate::round::RoundSnapshot;
use crate::status::StatusCard;

/// The conversation of the round the page shows.
#[derive(Debug, Serialize, TS)]
pub(crate) struct ConversationView {
    /// The round, by its instance, which a message the reviewer sends names.
    round: String,
    /// The messages in posting order.
    messages: Vec<ChatMessageView>,
    /// How many of the agent's replies the reviewer has not read.
    unread: usize,
    /// What the page marks read through once the chat showed the replies.
    read_through: u64,
    /// That the reviewer's waiting messages did not reach the agent, with Retry.
    card: Option<StatusCard>,
    /// Whether the reviewer can write: not in an earlier round, which offers Reset only.
    writable: bool,
}

/// One message of the conversation.
#[derive(Debug, Serialize, TS)]
struct ChatMessageView {
    id: String,
    author: Author,
    /// The text, as Markdown rendered.
    html: String,
    /// When the review received it, in milliseconds since the Unix epoch.
    posted_at_ms: Option<u64>,
    /// Where in the round the reviewer wrote it, as the rail names it: "Q2", "Design",
    /// "Conclusion"; `None` when it names no step, or one the round no longer shows.
    place: Option<String>,
    /// The passage of the round the reviewer quoted.
    quote: Option<String>,
    /// What became of a message of the reviewer's; an agent's reply has none.
    delivery: Option<Delivery>,
    /// Whether the reviewer has not read this reply.
    unread: bool,
}

impl ConversationView {
    /// The conversation of the round `round` shows, from the review threads `threads`; `None`
    /// when no round is running, and when the review tool cannot save the round.
    pub(crate) fn of(round: &RoundSnapshot, threads: &ThreadsSnapshot) -> Option<Self> {
        let (id, unit) = round.conversation_round()?;
        let conversation = threads.conversation(unit, id);
        Some(Self::new(round, conversation))
    }

    fn new(round: &RoundSnapshot, conversation: RoundConversation) -> Self {
        let undelivered =
            conversation
                .messages
                .iter()
                .find_map(|message| match &message.delivery {
                    Some(Delivery::NotDelivered { error }) => Some(error.clone()),
                    _ => None,
                });
        let writable = !round.earlier;
        Self {
            card: undelivered
                .filter(|_| writable && conversation.can_retry)
                .map(|error| StatusCard::undelivered_messages(&error, &conversation.round)),
            messages: conversation
                .messages
                .into_iter()
                .map(|message| ChatMessageView::new(round, message))
                .collect(),
            unread: conversation.unread,
            read_through: conversation.read_through,
            round: conversation.round,
            writable,
        }
    }
}

impl ChatMessageView {
    fn new(round: &RoundSnapshot, message: ConversationMessage) -> Self {
        Self {
            id: message.id.as_str().to_owned(),
            author: message.author,
            html: markdown(&message.text, 3),
            posted_at_ms: message.posted_at_ms,
            place: message
                .asked_under
                .as_ref()
                .and_then(|asked| place(round, asked)),
            quote: message.quote,
            delivery: message.delivery,
            unread: message.unread,
        }
    }
}

/// The step of the rail that `asked` names in `round`.
fn place(round: &RoundSnapshot, asked: &AskedUnder) -> Option<String> {
    match asked {
        AskedUnder::Question { question, .. } => round
            .number_of_question(question)
            .map(|number| format!("Q{number}")),
        AskedUnder::Design => Some("Design".to_owned()),
        AskedUnder::Conclusion { .. } => Some("Conclusion".to_owned()),
    }
}
