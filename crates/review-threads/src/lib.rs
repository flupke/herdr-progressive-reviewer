//! Persistent review conversations anchored to source code.

mod attention;
mod command;
mod message;
mod post;

use std::collections::{BTreeMap, BTreeSet};

use review_guide::DiffRangeAnchor;
use review_types::ReviewUnit;
use serde::{Deserialize, Serialize};

pub use attention::{Resolution, ThreadCounts};
pub use command::ThreadCommand;
pub use message::{Author, Message, MessageId};
pub use post::Post;

/// The stable identity of one review thread.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ThreadId(String);

/// A conversation that survives changes to, or deletion of, its anchor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewThread {
    pub id: ThreadId,
    pub anchor: DiffRangeAnchor,
    /// The original selected diff, retained when its source disappears.
    pub excerpt: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub resolution: Resolution,
    #[serde(default)]
    seen_reply_through: u64,
    #[serde(default)]
    seen_replies: BTreeSet<MessageId>,
}

impl ReviewThread {
    /// Path that originally held the selected source.
    pub fn path(&self) -> &str {
        self.anchor
            .new_path
            .as_deref()
            .or(self.anchor.old_path.as_deref())
            .unwrap_or("")
    }

    fn has_comments_after(&self, sequence: u64) -> bool {
        self.messages
            .iter()
            .any(|message| message.author == Author::Reviewer && message.sequence > sequence)
    }
}

/// Conversation history and each agent's read position within one logical review.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewThreads {
    pub review_unit: ReviewUnit,
    threads: Vec<ReviewThread>,
    sequence: u64,
    readers: BTreeMap<String, BTreeMap<ThreadId, u64>>,
}

impl ReviewThreads {
    pub fn new(review_unit: ReviewUnit) -> Self {
        Self {
            review_unit,
            threads: Vec::new(),
            sequence: 0,
            readers: BTreeMap::new(),
        }
    }

    pub fn threads(&self) -> &[ReviewThread] {
        &self.threads
    }

    pub fn thread(&self, id: &ThreadId) -> Option<&ReviewThread> {
        self.threads.iter().find(|thread| &thread.id == id)
    }

    pub fn thread_for_message(&self, id: &MessageId) -> Option<&ReviewThread> {
        self.threads
            .iter()
            .find(|thread| thread.messages.iter().any(|message| &message.id == id))
    }

    pub fn message(&self, id: &MessageId) -> Option<&Message> {
        self.thread_for_message(id)?
            .messages
            .iter()
            .find(|message| &message.id == id)
    }

    pub fn reply_count(&self) -> usize {
        self.threads
            .iter()
            .flat_map(|thread| &thread.messages)
            .filter(|message| message.author == Author::Agent)
            .count()
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    fn read_through(&self, reader: &str, thread: &ThreadId) -> u64 {
        self.readers
            .get(reader)
            .and_then(|positions| positions.get(thread))
            .copied()
            .unwrap_or_default()
    }

    /// Return complete conversations containing new reviewer comments.
    /// Agent replies supply context and never create additional agent work.
    pub fn new_messages(&self, reader: &str) -> Vec<ReviewThread> {
        self.threads
            .iter()
            .filter(|thread| thread.has_comments_after(self.read_through(reader, &thread.id)))
            .cloned()
            .collect()
    }

    pub fn has_new_messages(&self, reader: &str) -> bool {
        self.threads
            .iter()
            .any(|thread| thread.has_comments_after(self.read_through(reader, &thread.id)))
    }

    /// Record a retrieved snapshot without acknowledging comments posted afterward.
    pub fn mark_retrieved(&mut self, reader: &str, thread: &ThreadId, through: u64) {
        let position = self
            .readers
            .entry(reader.to_owned())
            .or_default()
            .entry(thread.clone())
            .or_default();
        *position = (*position).max(through.min(self.sequence));
    }

    /// Explicitly request that an agent revisit one existing conversation.
    pub fn retry(&mut self, reader: &str, thread: &ThreadId) -> Result<(), String> {
        if self.thread(thread).is_none() {
            return Err("The review thread no longer exists".into());
        }
        if let Some(positions) = self.readers.get_mut(reader) {
            positions.remove(thread);
        }
        Ok(())
    }

    /// Apply one immutable post. Repeating the same post cannot duplicate it.
    pub fn post(&mut self, post: Post) -> Result<MessageId, String> {
        if let Some(existing) = self.message(&post.message.id) {
            return if existing.same_content(&post.message)
                && self
                    .thread_for_message(&existing.id)
                    .map(|thread| &thread.id)
                    == Some(&post.thread_id)
            {
                Ok(existing.id.clone())
            } else {
                Err("This message identity is already used by a different post".into())
            };
        }
        if post.message.text.trim().is_empty() {
            return Err("A message cannot be empty".into());
        }
        self.append(post)
    }

    fn append(&mut self, mut post: Post) -> Result<MessageId, String> {
        let next = self.sequence.checked_add(1).ok_or("Too many messages")?;
        let id = post.message.id.clone();
        post.message.sequence = next;
        if let Some(source) = post.source {
            let (anchor, excerpt) = *source;
            if self.thread(&post.thread_id).is_some() {
                return Err("This review thread already exists".into());
            }
            self.threads.push(ReviewThread {
                id: post.thread_id,
                anchor,
                excerpt,
                messages: vec![post.message],
                resolution: Resolution::Open,
                seen_reply_through: 0,
                seen_replies: BTreeSet::new(),
            });
        } else {
            let thread = self
                .threads
                .iter_mut()
                .find(|thread| thread.id == post.thread_id)
                .ok_or("The review thread no longer exists")?;
            thread.messages.push(post.message);
        }
        self.sequence = next;
        Ok(id)
    }
}

#[cfg(test)]
mod tests;
