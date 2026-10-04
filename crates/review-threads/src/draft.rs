use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{
    Message, MessageId, Post, ReviewThread, ReviewThreads, ThreadId, ThreadSource, ThreadSubject,
};

/// Where an unposted editor belongs: a new thread in a file or a reply to an existing one.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum DraftTarget {
    File(String),
    Thread(ThreadId),
}

/// Saved separately from messages; restoring a draft never posts it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Draft {
    pub target: DraftTarget,
    pub source: Arc<ThreadSource>,
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

    /// A reply to a thread on code; the reviewer writes in a round conversation from the
    /// Explore page instead.
    pub fn reply(thread: &ReviewThread, reply_to: MessageId) -> Option<Self> {
        let ThreadSubject::Code(source) = &thread.subject else {
            return None;
        };
        Some(Self {
            target: DraftTarget::Thread(thread.id.clone()),
            source: source.clone(),
            reply_to: Some(reply_to),
            text: String::new(),
            id: Message::reviewer(String::new()).id,
            thread: thread.id.clone(),
        })
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
            subject: self
                .is_new_thread()
                .then(|| ThreadSubject::Code(self.source.clone())),
        }
    }
}

/// The drafts saved for one review. They are stored apart from its threads, so saving
/// one never rewrites posted messages, and a draft whose post reached the threads is
/// no longer a draft.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SavedDrafts {
    drafts: Vec<Draft>,
}

impl SavedDrafts {
    pub fn drafts(&self) -> &[Draft] {
        &self.drafts
    }

    /// Keep `draft`, replacing the saved draft of the same thread.
    pub fn save(&mut self, draft: Draft, threads: &ReviewThreads) -> Result<(), String> {
        if let DraftTarget::Thread(id) = &draft.target
            && threads.thread(id).is_none()
        {
            return Err("The draft's review thread no longer exists".into());
        }
        if threads.message(&draft.id).is_some() {
            return Err("This draft has already been posted".into());
        }
        self.keep(draft);
        Ok(())
    }

    /// Keep `draft` without checking it against the threads, replacing the saved draft of
    /// the same thread. Restoring older saved drafts must not lose one whose thread is gone.
    pub fn keep(&mut self, draft: Draft) {
        self.discard(&draft.thread);
        self.drafts.push(draft);
    }

    pub fn discard(&mut self, thread: &ThreadId) {
        self.drafts.retain(|draft| &draft.thread != thread);
    }

    /// Forget the drafts whose publication `threads` holds. Posting writes the threads
    /// before it discards the draft, so this also covers a post interrupted between them.
    pub fn forget_posted(&mut self, threads: &ReviewThreads) {
        self.drafts
            .retain(|draft| threads.message(&draft.id).is_none());
    }

    pub fn is_empty(&self) -> bool {
        self.drafts.is_empty()
    }

    /// Each draft's source, in draft order.
    pub fn sources_mut(&mut self) -> impl Iterator<Item = &mut Arc<ThreadSource>> {
        self.drafts.iter_mut().map(|draft| &mut draft.source)
    }
}

impl IntoIterator for SavedDrafts {
    type Item = Draft;
    type IntoIter = std::vec::IntoIter<Draft>;

    fn into_iter(self) -> Self::IntoIter {
        self.drafts.into_iter()
    }
}
