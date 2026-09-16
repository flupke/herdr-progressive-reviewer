use serde::{Deserialize, Serialize};

use crate::{Author, Message, MessageId, ReviewThread, ReviewThreads, ThreadId};

/// Conversation completion is independent of file review and agent activity.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    #[default]
    Open,
    Resolved,
}

/// Independent counts of conversations and attention for a logical review.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ThreadCounts {
    pub total: usize,
    pub open: usize,
    pub unread: usize,
    pub waiting: usize,
}

impl ThreadCounts {
    fn include(&mut self, other: Self) {
        self.total += other.total;
        self.open += other.open;
        self.unread += other.unread;
        self.waiting += other.waiting;
    }
}

impl ReviewThread {
    pub fn has_unread_replies(&self) -> bool {
        self.unread_replies().next().is_some()
    }

    pub fn has_unread_reply(&self, id: &MessageId) -> bool {
        self.unread_replies().any(|message| &message.id == id)
    }

    fn unread_replies(&self) -> impl Iterator<Item = &Message> {
        self.messages.iter().filter(|message| {
            message.author == Author::Agent
                && message.sequence > self.seen_reply_through
                && !self.seen_replies.contains(&message.id)
        })
    }

    pub fn is_waiting(&self) -> bool {
        let Some(comment) = self.last_comment() else {
            return false;
        };
        !self.messages.iter().any(|message| {
            message.author == Author::Agent
                && match &message.in_reply_to {
                    Some(id) => id == &comment.id,
                    None => message.sequence > comment.sequence,
                }
        })
    }

    fn counts(&self) -> ThreadCounts {
        ThreadCounts {
            total: 1,
            open: usize::from(self.resolution == Resolution::Open),
            unread: usize::from(self.has_unread_replies()),
            waiting: usize::from(self.is_waiting()),
        }
    }
}

impl ReviewThreads {
    /// Acknowledge individual replies without marking unseen earlier replies read.
    pub fn mark_replies_read(&mut self, ids: &[MessageId]) {
        for thread in &mut self.threads {
            for id in ids {
                if thread.has_unread_reply(id) {
                    thread.seen_replies.insert(id.clone());
                }
            }
        }
    }

    pub fn counts(&self) -> ThreadCounts {
        self.counts_for(|_| true)
    }

    pub fn counts_for(&self, include: impl Fn(&ReviewThread) -> bool) -> ThreadCounts {
        let mut counts = ThreadCounts::default();
        for thread in self.threads.iter().filter(|thread| include(thread)) {
            counts.include(thread.counts());
        }
        counts
    }

    pub fn set_resolution(&mut self, id: &ThreadId, resolution: Resolution) -> Result<(), String> {
        let thread = self
            .threads
            .iter_mut()
            .find(|thread| thread.id == *id)
            .ok_or("The review thread no longer exists")?;
        thread.resolution = resolution;
        Ok(())
    }

    /// Acknowledge only the displayed snapshot; a reply arriving later stays unread.
    pub fn mark_read(&mut self, id: &ThreadId, through: u64) -> Result<(), String> {
        let thread = self
            .threads
            .iter_mut()
            .find(|thread| thread.id == *id)
            .ok_or("The review thread no longer exists")?;
        thread.seen_reply_through = thread.seen_reply_through.max(through.min(self.sequence));
        Ok(())
    }
}
