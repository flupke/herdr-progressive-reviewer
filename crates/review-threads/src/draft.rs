use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{Message, MessageId, Post, ReviewThread, ReviewThreads, ThreadId, ThreadSource};

/// Where an unposted editor belongs: a new thread in a file or a reply to an existing one.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum DraftTarget {
    File(String),
    Thread(ThreadId),
}

/// Saved separately from messages; restoring a draft never posts it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Draft<S = Arc<ThreadSource>> {
    pub target: DraftTarget,
    pub source: S,
    pub reply_to: Option<MessageId>,
    pub text: String,
    id: MessageId,
    thread: ThreadId,
}

impl Draft {
    pub fn message_id(&self) -> &MessageId {
        &self.id
    }

    /// Starts a thread of its own rather than replying to an existing one.
    pub fn is_new_thread(&self) -> bool {
        matches!(self.target, DraftTarget::File(_))
    }

    /// Identifies the draft: a reply's existing thread, or the thread a new draft will start.
    pub fn thread_id(&self) -> &ThreadId {
        &self.thread
    }

    /// Preserve a divergent editor after another copy has published the original identity.
    pub fn renew_publication(&mut self) {
        self.id = Message::reviewer(String::new()).id;
        if self.is_new_thread() {
            self.thread = ThreadId(uuid::Uuid::new_v4().to_string());
        }
    }

    pub fn start(path: String, source: Arc<ThreadSource>) -> Self {
        Self {
            target: DraftTarget::File(path),
            source,
            reply_to: None,
            text: String::new(),
            id: Message::reviewer(String::new()).id,
            thread: ThreadId(uuid::Uuid::new_v4().to_string()),
        }
    }

    pub fn reply(thread: &ReviewThread, reply_to: MessageId) -> Self {
        Self {
            target: DraftTarget::Thread(thread.id.clone()),
            source: thread.source.clone(),
            reply_to: Some(reply_to),
            text: String::new(),
            id: Message::reviewer(String::new()).id,
            thread: thread.id.clone(),
        }
    }

    pub fn path(&self) -> &str {
        match &self.target {
            DraftTarget::File(path) => path,
            DraftTarget::Thread(_) => self
                .source
                .anchor
                .new_path
                .as_deref()
                .or(self.source.anchor.old_path.as_deref())
                .unwrap_or(""),
        }
    }

    /// Keep the same publication identity through persistence, retries and recovery.
    pub fn post(&self) -> Post {
        let mut message = Message::reviewer(self.text.clone());
        message.id = self.id.clone();
        Post {
            thread_id: self.thread.clone(),
            message,
            source: self.is_new_thread().then(|| self.source.clone()),
        }
    }
}

impl<S> Draft<S> {
    pub(super) fn try_map_source<T, E>(
        self,
        map: &mut impl FnMut(S) -> Result<T, E>,
    ) -> Result<Draft<T>, E> {
        Ok(Draft {
            target: self.target,
            source: map(self.source)?,
            reply_to: self.reply_to,
            text: self.text,
            id: self.id,
            thread: self.thread,
        })
    }
}

impl ReviewThreads {
    pub fn drafts(&self) -> &[Draft] {
        &self.drafts
    }

    pub fn save_draft(&mut self, draft: Draft) -> Result<(), String> {
        if let DraftTarget::Thread(id) = &draft.target
            && self.thread(id).is_none()
        {
            return Err("The draft's review thread no longer exists".into());
        }
        if self.message(&draft.id).is_some() {
            return Err("This draft has already been posted".into());
        }
        self.discard_draft(&draft.thread);
        self.drafts.push(draft);
        Ok(())
    }

    pub fn discard_draft(&mut self, thread: &ThreadId) {
        self.drafts.retain(|draft| &draft.thread != thread);
    }

    pub(super) fn discard_posted_draft(&mut self, id: &MessageId) {
        self.drafts.retain(|draft| &draft.id != id);
    }
}
