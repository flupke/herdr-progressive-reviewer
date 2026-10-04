//! The round conversation: the review thread attached to one Explore round, where the
//! reviewer talks with the agent beside the round's questions without answering them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Message, MessageId, Post, ReviewThread, ReviewThreads, ThreadId, ThreadSubject};

/// Where in its round the reviewer wrote a message: under a question, by its identity and
/// version, or at a stage that shows no question.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum AskedUnder {
    /// Version `version` of the question `question`.
    Question { question: String, version: u32 },
    /// The design explanation of the round's first turn.
    Design,
    /// The conclusion that the agent's turn with request `conclusion` posted.
    Conclusion { conclusion: String },
}

impl ThreadId {
    /// The conversation of Explore round `round`: one per round, whichever message starts it.
    fn of_round(round: &str) -> Self {
        Self(format!("explore-round-{round}"))
    }
}

impl Post {
    /// A reviewer message in the conversation of Explore round `round`, written under
    /// `asked_under`, quoting `quote`. The round's first message starts its conversation.
    pub fn to_round(
        round: &str,
        text: String,
        asked_under: Option<AskedUnder>,
        quote: Option<String>,
    ) -> Self {
        let mut message = Message::reviewer(text);
        message.asked_under = asked_under;
        message.quote = quote;
        Self {
            thread_id: ThreadId::of_round(round),
            message,
            subject: Some(ThreadSubject::Round {
                round: round.to_owned(),
            }),
        }
    }

    /// The same message under the identity `id`, which its sender chose, so that sending it
    /// again, after a reply that did not arrive, posts it once.
    #[must_use]
    pub fn with_id(mut self, id: MessageId) -> Self {
        self.message.id = id;
        self
    }
}

impl ReviewThread {
    /// The Explore round whose conversation this thread is.
    pub fn round(&self) -> Option<&str> {
        match &self.subject {
            ThreadSubject::Round { round } => Some(round),
            ThreadSubject::Code(_) => None,
        }
    }
}

impl ReviewThreads {
    /// The conversation of Explore round `round`, once its first message started it.
    pub fn round_conversation(&self, round: &str) -> Option<&ReviewThread> {
        self.thread(&ThreadId::of_round(round))
    }
}

#[cfg(test)]
#[path = "round.tests.rs"]
mod tests;
