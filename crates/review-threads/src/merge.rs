//! One review's threads kept in two documents, such as by two jj workspaces before they shared
//! their review state, merged into one.

use std::collections::HashMap;

use crate::{Message, MessageId, Resolution, ReviewThreads};

impl ReviewThreads {
    /// Add the threads and messages of `other`, a document of the same review kept apart. The
    /// messages this document lacks take sequence numbers after its own, in `other`'s posting
    /// order, and what `other` answered and read moves with them, so its pending comments stay
    /// pending and its answered ones answered. Merging the same document again changes nothing.
    ///
    /// A thread that both documents continued apart, which only a reviewer of an earlier build
    /// still running gives, has `other`'s new messages after its own: whether it waits for an
    /// answer follows the last of them, as for any thread.
    pub fn merge(&mut self, other: Self) -> Result<(), String> {
        if other.review_unit != self.review_unit {
            return Err("The threads belong to another review".into());
        }
        let known: HashMap<MessageId, u64> = self
            .threads
            .iter()
            .flat_map(|thread| &thread.messages)
            .map(|message| (message.id.clone(), message.sequence))
            .collect();
        let mut arriving = other
            .threads
            .iter()
            .flat_map(|thread| &thread.messages)
            .filter(|message| !known.contains_key(&message.id))
            .map(|message| (message.sequence, message.id.clone()))
            .collect::<Vec<_>>();
        arriving.sort();
        let mut sequence = self.sequence;
        let mut renumbered = HashMap::new();
        for (_, id) in arriving {
            sequence = sequence.checked_add(1).ok_or("Too many messages")?;
            renumbered.insert(id, sequence);
        }
        let merged = |message: &Message| {
            renumbered
                .get(&message.id)
                .or_else(|| known.get(&message.id))
                .copied()
                .unwrap_or_default()
        };
        for mut thread in other.threads {
            // A position of `other` in this thread becomes the merged sequence of the latest of
            // its messages it covered: positions of one thread never cover another's messages.
            let translate = |position: u64| {
                thread
                    .messages
                    .iter()
                    .filter(|message| message.sequence <= position)
                    .map(merged)
                    .max()
                    .unwrap_or_default()
            };
            let answered = other
                .answered
                .get(&thread.id)
                .map(|&position| translate(position));
            let seen_reply_through = translate(thread.seen_reply_through);
            let mut arrived = thread
                .messages
                .drain(..)
                .filter(|message| renumbered.contains_key(&message.id))
                .map(|mut message| {
                    message.sequence = renumbered[&message.id];
                    message
                })
                .collect::<Vec<_>>();
            arrived.sort_by_key(|message| message.sequence);
            let id = thread.id.clone();
            if let Some(kept) = self.threads.iter_mut().find(|kept| kept.id == thread.id) {
                if !arrived.is_empty() && thread.resolution == Resolution::Open {
                    // A comment that arrives may wait for an answer, which a resolved thread
                    // would never bring to the agent.
                    kept.resolution = Resolution::Open;
                }
                kept.messages.extend(arrived);
                kept.seen_reply_through = kept.seen_reply_through.max(seen_reply_through);
                kept.seen_replies.append(&mut thread.seen_replies);
            } else {
                thread.messages = arrived;
                thread.seen_reply_through = seen_reply_through;
                self.threads.push(thread);
            }
            if let Some(position) = answered {
                let kept = self.answered.entry(id).or_default();
                *kept = (*kept).max(position);
            }
        }
        self.sequence = sequence;
        Ok(())
    }
}

#[cfg(test)]
#[path = "merge.tests.rs"]
mod tests;
