//! Every draft the reviewer is composing, across the reviews and viewers of one reviewer.
//!
//! A draft is started, parked, activated, recovered from a saved review, reconciled with
//! posted messages, posted, cancelled or renewed only here. Each change returns the thread
//! commands that persist it; views only ask which drafts exist and which editor they hold.

use std::collections::BTreeMap;

use comment_editor::{CommentEditor, KeymapSetting};
use review_threads::{
    Draft, MessageId, ReviewThread, ReviewThreads, SavedDrafts, ThreadCommand, ThreadId,
};
use review_types::ReviewUnit;
use ui_keys::Key;

/// A draft's identity while it is composed. It survives renewal, unlike its publication.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DraftId(u64);

/// The drafts of every review this reviewer has shown.
#[derive(Default)]
pub struct Drafts {
    keymap: KeymapSetting,
    next: u64,
    slots: BTreeMap<DraftId, Slot>,
}

/// An editor whose text is not part of the review thread yet.
pub struct OpenDraft {
    unit: ReviewUnit,
    draft: Draft,
    editor: CommentEditor,
    posting: bool,
}

/// What a submitted draft became.
#[derive(Debug, Eq, PartialEq)]
pub enum Submission {
    /// The text is being published; the draft stays until the post is acknowledged.
    Posting(ThreadCommand),
    /// Blank text cancels the draft instead of posting it.
    Cancelled(ThreadCommand),
}

enum Slot {
    Open(Box<OpenDraft>),
    /// A publication identity that no longer belongs to a draft. Saved reviews that still
    /// list it are stale and cannot bring it back.
    Closed {
        unit: ReviewUnit,
        message: MessageId,
        ending: Ending,
    },
}

/// Why a draft stopped being one.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Ending {
    /// Its text reached the review thread.
    Posted,
    /// The reviewer cancelled it, or left it blank.
    Dropped,
}

impl OpenDraft {
    pub fn unit(&self) -> &ReviewUnit {
        &self.unit
    }

    pub fn draft(&self) -> &Draft {
        &self.draft
    }

    pub fn editor(&self) -> &CommentEditor {
        &self.editor
    }

    /// The text is published and waits for acknowledgement; it cannot change meanwhile.
    pub fn is_posting(&self) -> bool {
        self.posting
    }

    fn is_blank(&self) -> bool {
        self.editor.text().trim().is_empty()
    }

    fn save(&self) -> ThreadCommand {
        ThreadCommand::SaveDraft {
            review_unit: self.unit.clone(),
            draft: self.draft.clone(),
        }
    }

    fn edit(&mut self, change: impl FnOnce(&mut CommentEditor)) -> Option<ThreadCommand> {
        if self.posting {
            return None;
        }
        change(&mut self.editor);
        let text = self.editor.text();
        if self.draft.text == text {
            return None;
        }
        self.draft.text = text;
        Some(self.save())
    }
}

impl Slot {
    fn open(&self) -> Option<&OpenDraft> {
        match self {
            Self::Open(draft) => Some(draft.as_ref()),
            Self::Closed { .. } => None,
        }
    }

    fn identifies(&self, unit: &ReviewUnit, id: &MessageId) -> bool {
        match self {
            Self::Open(draft) => &draft.unit == unit && draft.draft.message_id() == id,
            Self::Closed {
                unit: closed,
                message,
                ..
            } => closed == unit && message == id,
        }
    }
}

impl Drafts {
    /// New editors follow `keymap`, which the reviewer shares with every other editor.
    pub fn use_keymap(&mut self, keymap: KeymapSetting) {
        self.keymap = keymap;
    }

    /// Begin composing `draft` in an empty editor.
    pub fn start(&mut self, unit: ReviewUnit, draft: Draft) -> DraftId {
        let editor = CommentEditor::new("", &self.keymap);
        self.insert(unit, draft, editor)
    }

    /// Begin a reply to `thread` answering `reply_to`, unless one is already composed or
    /// the thread is a round conversation, which the reviewer writes in from the Explore page.
    pub fn start_reply(
        &mut self,
        unit: ReviewUnit,
        thread: &ReviewThread,
        reply_to: MessageId,
    ) -> Option<DraftId> {
        if self.for_thread(&unit, &thread.id).is_some() {
            return None;
        }
        Some(self.start(unit, Draft::reply(thread, reply_to)?))
    }

    pub fn get(&self, id: DraftId) -> Option<&OpenDraft> {
        self.slots.get(&id).and_then(Slot::open)
    }

    /// The draft that replies to, or will start, `thread`.
    pub fn for_thread(&self, unit: &ReviewUnit, thread: &ThreadId) -> Option<DraftId> {
        self.find(unit, |open| open.draft.thread_id() == thread)
    }

    /// The drafts of one review, oldest first.
    pub fn in_review(&self, unit: &ReviewUnit) -> impl Iterator<Item = (DraftId, &OpenDraft)> {
        let unit = unit.clone();
        self.slots
            .iter()
            .filter_map(|(id, slot)| slot.open().map(|open| (*id, open)))
            .filter(move |(_, open)| open.unit == unit)
    }

    /// The message a draft became once its post reached the review.
    pub fn posted_as(&self, id: DraftId) -> Option<&MessageId> {
        match self.slots.get(&id)? {
            Slot::Closed {
                message,
                ending: Ending::Posted,
                ..
            } => Some(message),
            Slot::Open(_) | Slot::Closed { .. } => None,
        }
    }

    pub fn input(&mut self, id: DraftId, key: Key) -> Option<ThreadCommand> {
        self.open_mut(id)?.edit(|editor| editor.input(key))
    }

    pub fn paste(&mut self, id: DraftId, text: &str) -> Option<ThreadCommand> {
        self.open_mut(id)?.edit(|editor| editor.paste(text))
    }

    /// The draft lost focus. A blank editor has nothing to keep.
    pub fn park(&mut self, id: DraftId) {
        if self
            .get(id)
            .is_some_and(|open| !open.posting && open.is_blank())
        {
            self.close(id, Ending::Dropped);
        }
    }

    /// Post the text, or cancel the draft when there is nothing to post.
    pub fn submit(&mut self, id: DraftId) -> Option<Submission> {
        let open = self.get(id)?;
        if open.posting {
            return None;
        }
        if open.is_blank() {
            return self.cancel(id).map(Submission::Cancelled);
        }
        let open = self.open_mut(id)?;
        open.draft.text = open.editor.text();
        open.posting = true;
        Some(Submission::Posting(ThreadCommand::Post {
            review_unit: open.unit.clone(),
            post: open.draft.post(),
        }))
    }

    /// Drop the draft and its saved copy. A draft being posted can no longer be cancelled.
    pub fn cancel(&mut self, id: DraftId) -> Option<ThreadCommand> {
        let open = self.get(id).filter(|open| !open.posting)?;
        let command = ThreadCommand::DiscardDraft {
            review_unit: open.unit.clone(),
            thread_id: open.draft.thread_id().clone(),
        };
        self.close(id, Ending::Dropped);
        Some(command)
    }

    /// The review thread holds the post of `message`.
    pub fn post_succeeded(&mut self, unit: &ReviewUnit, message: &MessageId) {
        let Some(id) = self.posting(unit, message) else {
            return;
        };
        let text = self.get(id).map(|open| open.draft.text.clone());
        self.settle(id, text.as_deref().unwrap_or_default());
    }

    /// The post of `message` failed; the reviewer can edit and post again.
    pub fn post_failed(&mut self, unit: &ReviewUnit, message: &MessageId) {
        if let Some(open) = self.posting(unit, message).and_then(|id| self.open_mut(id)) {
            open.posting = false;
        }
    }

    /// Reopen the drafts `saved` for `unit` that this reviewer does not hold yet.
    pub fn recover(&mut self, unit: &ReviewUnit, saved: &SavedDrafts) {
        for draft in saved.drafts() {
            let known = self
                .slots
                .values()
                .any(|slot| slot.identifies(unit, draft.message_id()))
                || self.for_thread(unit, draft.thread_id()).is_some();
            if known || draft.text.trim().is_empty() {
                continue;
            }
            let editor = CommentEditor::new(&draft.text, &self.keymap);
            self.insert(unit.clone(), draft.clone(), editor);
        }
    }

    /// Settle the drafts whose publication `book` already holds.
    pub fn reconcile(&mut self, book: &ReviewThreads) -> Vec<ThreadCommand> {
        let published = self
            .in_review(&book.review_unit)
            .filter_map(|(id, open)| {
                book.message(open.draft.message_id())
                    .map(|message| (id, message.text.clone()))
            })
            .collect::<Vec<_>>();
        published
            .into_iter()
            .filter_map(|(id, text)| self.settle(id, &text))
            .collect()
    }

    /// The one decision between "posted" and "still a draft": a draft whose publication
    /// carries its exact text is done; one edited since keeps its text under a new identity.
    fn settle(&mut self, id: DraftId, published: &str) -> Option<ThreadCommand> {
        let open = self.get(id)?;
        if open.editor.text() == published {
            self.close(id, Ending::Posted);
            return None;
        }
        let retired = Slot::Closed {
            unit: open.unit.clone(),
            message: open.draft.message_id().clone(),
            ending: Ending::Posted,
        };
        self.push(retired);
        let open = self.open_mut(id)?;
        open.draft.renew_publication();
        open.draft.text = open.editor.text();
        open.posting = false;
        Some(open.save())
    }

    fn posting(&self, unit: &ReviewUnit, message: &MessageId) -> Option<DraftId> {
        self.find(unit, |open| {
            open.posting && open.draft.message_id() == message
        })
    }

    fn find(&self, unit: &ReviewUnit, matches: impl Fn(&OpenDraft) -> bool) -> Option<DraftId> {
        self.in_review(unit)
            .find(|(_, open)| matches(open))
            .map(|(id, _)| id)
    }

    fn open_mut(&mut self, id: DraftId) -> Option<&mut OpenDraft> {
        match self.slots.get_mut(&id)? {
            Slot::Open(open) => Some(open.as_mut()),
            Slot::Closed { .. } => None,
        }
    }

    fn close(&mut self, id: DraftId, ending: Ending) {
        if let Some(Slot::Open(open)) = self.slots.remove(&id) {
            self.slots.insert(
                id,
                Slot::Closed {
                    unit: open.unit,
                    message: open.draft.message_id().clone(),
                    ending,
                },
            );
        }
    }

    fn insert(&mut self, unit: ReviewUnit, draft: Draft, editor: CommentEditor) -> DraftId {
        self.push(Slot::Open(Box::new(OpenDraft {
            unit,
            draft,
            editor,
            posting: false,
        })))
    }

    fn push(&mut self, slot: Slot) -> DraftId {
        let id = DraftId(self.next);
        self.next += 1;
        self.slots.insert(id, slot);
        id
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
