//! Unposted editors survive conversation and review navigation.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use comment_editor::{CommentEditor, EditorKeymap};
use review_threads::{Draft, DraftTarget, MessageId, ReviewThreads, ThreadCommand, ThreadId};
use review_types::ReviewUnit;
use ui_actions::Action;
use ui_events::ThreadPostFinished;

use super::{Comments, EditingComment};

#[derive(Default)]
pub(super) struct Drafts {
    parked: HashMap<(ReviewUnit, DraftTarget), EditingComment>,
    file_editor: Option<DraftTarget>,
    recovered: HashSet<(ReviewUnit, MessageId)>,
    cancelled: Rc<RefCell<HashSet<(ReviewUnit, MessageId)>>>,
}

impl Drafts {
    pub(super) fn share_cancellations(&mut self, other: &Self) {
        self.cancelled = Rc::clone(&other.cancelled);
    }

    pub(super) fn remember_cancellation(&self, unit: &ReviewUnit, draft: &Draft) {
        self.cancelled
            .borrow_mut()
            .insert((unit.clone(), draft.message_id().clone()));
    }

    fn reconcile_posts(&mut self, book: &ReviewThreads, actions: &mut Vec<Action>) {
        self.parked.retain(|(unit, _), editing| {
            unit != &book.review_unit || editing.retain_after_posts(book, actions)
        });
    }

    pub(super) fn recover(
        &mut self,
        book: &ReviewThreads,
        keymap: EditorKeymap,
        active: Option<&DraftTarget>,
    ) {
        for draft in book.drafts() {
            // A cached book cannot suppress later drafts or resurrect cancelled ones.
            let identity = (book.review_unit.clone(), draft.message_id().clone());
            let unseen = self.recovered.insert(identity.clone());
            if !unseen
                || active == Some(&draft.target)
                || self.cancelled.borrow().contains(&identity)
            {
                continue;
            }
            self.parked
                .entry((book.review_unit.clone(), draft.target.clone()))
                .or_insert_with(|| EditingComment {
                    posting: None,
                    editor: CommentEditor::new(&draft.text, keymap),
                    draft: draft.clone(),
                });
        }
    }

    pub(super) fn post_finished(&mut self, event: &ThreadPostFinished) {
        let key = self
            .parked
            .iter()
            .find(|((unit, _), editing)| {
                unit == &event.review_unit && editing.posting.as_ref() == Some(&event.message_id)
            })
            .map(|(key, _)| key.clone());
        if let Some(key) = key {
            if event.result.is_ok() {
                self.parked.remove(&key);
            } else if let Some(editing) = self.parked.get_mut(&key) {
                editing.posting = None;
            }
        }
    }
}

impl EditingComment {
    fn retain_after_posts(&mut self, book: &ReviewThreads, actions: &mut Vec<Action>) -> bool {
        let Some(posted) = book.message(self.draft.message_id()) else {
            return true;
        };
        let text = self.editor.text();
        if text == posted.text {
            return false;
        }
        self.draft.renew_publication();
        self.draft.text = text;
        self.posting = None;
        actions.push(Action::Thread(ThreadCommand::SaveDraft {
            review_unit: book.review_unit.clone(),
            draft: self.draft.clone(),
        }));
        true
    }
}

impl Comments {
    pub(super) fn reconcile_posts(&mut self, book: &ReviewThreads) -> Vec<Action> {
        let mut actions = Vec::new();
        self.drafts.reconcile_posts(book, &mut actions);
        if let Some(editing) = &mut self.editing
            && !editing.retain_after_posts(book, &mut actions)
        {
            self.selected = Some(editing.draft.message_id().clone());
            self.editing = None;
        }
        actions
    }

    pub(super) fn draft_paths(&self) -> impl Iterator<Item = &str> {
        self.editing
            .iter()
            .chain(self.drafts.parked.iter().filter_map(|((unit, _), editor)| {
                self.book
                    .as_ref()
                    .filter(|book| &book.review_unit == unit)
                    .map(|_| editor)
            }))
            .filter_map(|editor| match &editor.draft.target {
                DraftTarget::File(path) => Some(path.as_str()),
                DraftTarget::Thread(_) => None,
            })
    }

    pub(crate) fn park_editor(&mut self) {
        if let Some(book) = &self.book
            && let Some(editing) = self.editing.take()
        {
            self.drafts.parked.insert(
                (book.review_unit.clone(), editing.draft.target.clone()),
                editing,
            );
        }
    }

    pub(in crate::comments) fn activate_editor(&mut self, target: DraftTarget) {
        if self
            .book
            .as_ref()
            .zip(self.editing.as_ref())
            .is_some_and(|(_, editing)| editing.draft.target == target)
        {
            return;
        }
        self.park_editor();
        if let Some(book) = &self.book {
            self.editing = self
                .drafts
                .parked
                .remove(&(book.review_unit.clone(), target));
        }
    }

    pub(crate) fn enter_conversations(&mut self) {
        self.drafts.file_editor = self
            .book
            .as_ref()
            .zip(self.editing.as_ref())
            .map(|(_, editing)| editing.draft.target.clone());
        self.park_editor();
    }

    pub(crate) fn leave_conversations(&mut self) {
        self.park_editor();
        if let Some(target) = self.drafts.file_editor.take() {
            self.activate_editor(target);
        }
    }

    pub(crate) fn restore_thread_editor(&mut self, id: ThreadId) {
        self.activate_editor(DraftTarget::Thread(id));
    }

    pub(crate) fn restore_file_editor(&mut self, path: &str) {
        let original = self
            .draft_paths()
            .find(|original| self.paths.resolve(original) == path)
            .unwrap_or(path)
            .to_owned();
        self.activate_editor(DraftTarget::File(original));
    }
}
