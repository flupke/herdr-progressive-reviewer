//! Persistent review conversations anchored to source code.

mod attention;
mod command;
mod draft;
mod message;
mod paths;
mod post;
mod source;

use std::collections::{BTreeMap, BTreeSet};

use review_types::ReviewUnit;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub use attention::{Resolution, ThreadCounts};
pub use command::ThreadCommand;
pub use draft::{Draft, DraftTarget};
pub use message::{Author, Message, MessageId};
pub use paths::ThreadPaths;
pub use post::Post;
pub use source::ThreadSource;

/// The stable identity of one review thread.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ThreadId(String);

/// A conversation that survives changes to, or deletion of, its anchor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewThread<S = Arc<ThreadSource>> {
    pub id: ThreadId,
    #[serde(flatten)]
    pub source: S,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub resolution: Resolution,
    #[serde(default)]
    seen_reply_through: u64,
    #[serde(default)]
    seen_replies: BTreeSet<MessageId>,
}

impl ReviewThread {
    /// The snapshot boundary returned with a conversation for an agent reply.
    pub fn last_comment(&self) -> Option<&Message> {
        self.messages
            .iter()
            .rev()
            .find(|message| message.author == Author::Reviewer)
    }

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

/// Conversation history and completed work shared by every recipient of a logical review.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(bound(deserialize = "S: Deserialize<'de>"))]
pub struct ReviewThreads<S = Arc<ThreadSource>> {
    pub review_unit: ReviewUnit,
    #[serde(default)]
    drafts: Vec<Draft<S>>,
    threads: Vec<ReviewThread<S>>,
    sequence: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    readers: BTreeMap<String, BTreeMap<ThreadId, u64>>,
    #[serde(default)]
    answered: BTreeMap<ThreadId, u64>,
}

impl ReviewThreads {
    pub fn new(review_unit: ReviewUnit) -> Self {
        Self {
            review_unit,
            drafts: Vec::new(),
            threads: Vec::new(),
            sequence: 0,
            readers: BTreeMap::new(),
            answered: BTreeMap::new(),
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

    fn pending(&self, thread: &ReviewThread) -> bool {
        thread.resolution == Resolution::Open
            && thread.is_waiting()
            && thread.has_comments_after(self.answered.get(&thread.id).copied().unwrap_or_default())
    }

    /// Completed answers belong to the review, including after a recipient handoff.
    pub fn migrate_answered_positions(&mut self) {
        for positions in std::mem::take(&mut self.readers).into_values() {
            for (thread, through) in positions {
                let position = self.answered.entry(thread).or_default();
                *position = (*position).max(through);
            }
        }
        // Older Retry actions could erase a recipient's cursor. Exact answer
        // boundaries still prove completion even when that cursor is missing.
        for thread in &self.threads {
            let through = thread
                .messages
                .iter()
                .filter(|message| message.author == Author::Agent)
                .filter_map(|message| message.in_reply_to.as_ref())
                .filter_map(|id| {
                    thread
                        .messages
                        .iter()
                        .find(|message| message.id == *id && message.author == Author::Reviewer)
                })
                .map(|message| message.sequence)
                .max()
                .unwrap_or_default();
            let position = self.answered.entry(thread.id.clone()).or_default();
            *position = (*position).max(through);
        }
    }

    /// Discard retrieval cursors that cannot prove an answer was saved.
    pub fn recover_retrieved_comments(&mut self) {
        for positions in self.readers.values_mut() {
            for (id, through) in positions {
                let answered = self
                    .threads
                    .iter()
                    .find(|thread| thread.id == *id)
                    .and_then(|thread| {
                        thread
                            .messages
                            .iter()
                            .rev()
                            .find(|message| message.author == Author::Agent)
                    })
                    .map_or(0, |message| message.sequence);
                // A read at or after the last answer may have fetched comments
                // that arrived while that answer was being written. The old
                // format has no snapshot boundary. Pending work must also have
                // an unanswered comment, as shown by the conversation itself.
                if *through >= answered {
                    *through = 0;
                }
            }
        }
    }

    /// Return complete conversations containing reviewer comments awaiting a reply.
    /// Agent replies supply context and never create additional agent work.
    pub fn new_messages(&self) -> Vec<ReviewThread> {
        self.threads
            .iter()
            .filter(|thread| self.pending(thread))
            .cloned()
            .collect()
    }

    pub fn has_new_messages(&self) -> bool {
        self.threads.iter().any(|thread| self.pending(thread))
    }

    /// Latest pending reviewer comment; agent replies do not advance this position.
    pub fn pending_comment_sequence(&self) -> Option<u64> {
        self.threads
            .iter()
            .filter(|thread| self.pending(thread))
            .filter_map(ReviewThread::last_comment)
            .map(|message| message.sequence)
            .max()
    }

    /// Append an answer and acknowledge its exact snapshot in the same stored update.
    pub fn answer(&mut self, post: Post) -> Result<MessageId, String> {
        if post.message.author != Author::Agent {
            return Err("Only an agent reply can acknowledge comments".into());
        }
        let through = self
            .thread(&post.thread_id)
            .and_then(|thread| {
                thread.messages.iter().find(|message| {
                    Some(&message.id) == post.message.in_reply_to.as_ref()
                        && message.author == Author::Reviewer
                })
            })
            .map(|message| message.sequence)
            .ok_or("in_reply_to must identify a reviewer comment in this thread")?;
        let thread = post.thread_id.clone();
        let id = self.post(post)?;
        let position = self.answered.entry(thread).or_default();
        *position = (*position).max(through);
        Ok(id)
    }

    /// Retry delivery of unanswered work without reopening completed conversations.
    pub fn retry(&self, thread: &ThreadId) -> Result<(), String> {
        let thread = self
            .thread(thread)
            .ok_or("The review thread no longer exists")?;
        if !self.pending(thread) {
            return Err("This thread has no unanswered, unresolved comments".into());
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
            if self.thread(&post.thread_id).is_some() {
                return Err("This review thread already exists".into());
            }
            self.threads.push(ReviewThread {
                id: post.thread_id,
                source,
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
        self.discard_posted_draft(&id);
        Ok(id)
    }
}

#[cfg(test)]
mod tests;
