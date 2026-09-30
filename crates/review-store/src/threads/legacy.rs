//! Thread documents before version 4 tracked delivery with a retrieval cursor per
//! recipient. Current builds track which comments each thread's answers cover for the
//! whole review, so loading such a document derives that record from the cursors and
//! the conversations.

use std::collections::BTreeMap;

use review_threads::{Author, MessageId, ThreadId};
use serde::Deserialize;
use serde_json::Value;

/// The delivery state of a version 2 or 3 thread document. It is read apart from
/// `ReviewThreads`, which no longer holds recipient cursors, and it reads only the
/// message fields that delivery depends on.
#[derive(Deserialize)]
struct RecipientCursors {
    threads: Vec<CursorThread>,
    #[serde(default)]
    readers: BTreeMap<String, BTreeMap<ThreadId, u64>>,
    #[serde(default)]
    answered: BTreeMap<ThreadId, u64>,
}

#[derive(Deserialize)]
struct CursorThread {
    id: ThreadId,
    messages: Vec<CursorMessage>,
}

#[derive(Deserialize)]
struct CursorMessage {
    id: MessageId,
    author: Author,
    #[serde(default)]
    in_reply_to: Option<MessageId>,
    sequence: u64,
}

impl CursorThread {
    fn last_reply_sequence(&self) -> u64 {
        self.messages
            .iter()
            .rev()
            .find(|message| message.author == Author::Agent)
            .map_or(0, |message| message.sequence)
    }

    /// The latest reviewer comment that an agent reply names as the comment it answers.
    fn answered_through(&self) -> u64 {
        self.messages
            .iter()
            .filter(|message| message.author == Author::Agent)
            .filter_map(|message| message.in_reply_to.as_ref())
            .filter_map(|id| {
                self.messages
                    .iter()
                    .find(|message| message.id == *id && message.author == Author::Reviewer)
            })
            .map(|message| message.sequence)
            .max()
            .unwrap_or_default()
    }
}

impl RecipientCursors {
    /// Version 2 advanced a cursor when comments were fetched, not when they were
    /// answered. Discard the cursors that cannot prove an answer was saved.
    fn recover_retrieved_comments(&mut self) {
        for positions in self.readers.values_mut() {
            for (id, through) in positions {
                let answered = self
                    .threads
                    .iter()
                    .find(|thread| thread.id == *id)
                    .map_or(0, CursorThread::last_reply_sequence);
                // A read at or after the last answer may have fetched comments that
                // arrived while that answer was being written. The old format has no
                // snapshot boundary. Pending work must also have an unanswered comment,
                // as shown by the conversation itself.
                if *through >= answered {
                    *through = 0;
                }
            }
        }
    }

    /// Completed answers belong to the review, including after a recipient handoff.
    fn into_answered(mut self) -> BTreeMap<ThreadId, u64> {
        for positions in std::mem::take(&mut self.readers).into_values() {
            for (thread, through) in positions {
                let position = self.answered.entry(thread).or_default();
                *position = (*position).max(through);
            }
        }
        // Older Retry actions could erase a recipient's cursor. Exact answer boundaries
        // still prove completion even when that cursor is missing.
        for thread in &self.threads {
            let position = self.answered.entry(thread.id.clone()).or_default();
            *position = (*position).max(thread.answered_through());
        }
        self.answered
    }
}

/// Replace the recipient cursors of a version 2 or 3 `conversations` value with the
/// answered positions they prove.
pub(super) fn migrate(conversations: &mut Value, version: u8) -> serde_json::Result<()> {
    let mut cursors = RecipientCursors::deserialize(&*conversations)?;
    if version == 2 {
        cursors.recover_retrieved_comments();
    }
    let answered = serde_json::to_value(cursors.into_answered())?;
    if let Value::Object(fields) = conversations {
        fields.remove("readers");
        fields.insert("answered".into(), answered);
    }
    Ok(())
}
