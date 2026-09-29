//! Unposted editors survive conversation and review navigation. A file can hold several new
//! thread drafts; each one is identified by the thread it will start.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;
use std::rc::Rc;

use comment_editor::{CommentEditor, KeymapSetting};
use review_threads::{Draft, DraftTarget, MessageId, ReviewThreads, ThreadCommand, ThreadId};
use review_types::ReviewUnit;
use ui_actions::Action;
use ui_events::ThreadPostFinished;

use super::{Comments, EditingComment};
use crate::LoadedDocument;
use crate::comment_layout::draft_rows;

#[derive(Default)]
pub(super) struct Drafts {
    parked: HashMap<(ReviewUnit, ThreadId), EditingComment>,
    file_editor: Option<ThreadId>,
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
        // A divergent new-thread draft is renewed under another thread, so rebuild its key.
        let file_editor = &mut self.file_editor;
        self.parked = std::mem::take(&mut self.parked)
            .into_iter()
            .filter_map(|((unit, thread), mut editing)| {
                if unit == book.review_unit && !editing.retain_after_posts(book, actions) {
                    return None;
                }
                let renewed = editing.draft.thread_id().clone();
                if file_editor.as_ref() == Some(&thread) {
                    *file_editor = Some(renewed.clone());
                }
                Some(((unit, renewed), editing))
            })
            .collect();
    }

    pub(super) fn recover(
        &mut self,
        book: &ReviewThreads,
        keymap: &KeymapSetting,
        active: Option<&ThreadId>,
    ) {
        for draft in book.drafts() {
            // A cached book cannot suppress later drafts or resurrect cancelled ones.
            let identity = (book.review_unit.clone(), draft.message_id().clone());
            let unseen = self.recovered.insert(identity.clone());
            if !unseen
                || draft.text.trim().is_empty()
                || active == Some(draft.thread_id())
                || self.cancelled.borrow().contains(&identity)
            {
                continue;
            }
            self.parked
                .entry((book.review_unit.clone(), draft.thread_id().clone()))
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

    /// New-thread editors of the current review that do not have focus.
    pub(crate) fn parked_file_editors(&self) -> impl Iterator<Item = &EditingComment> {
        self.drafts
            .parked
            .iter()
            .filter(|((unit, _), _)| {
                self.book
                    .as_ref()
                    .is_some_and(|book| &book.review_unit == unit)
            })
            .map(|(_, editing)| editing)
            .filter(|editing| editing.draft.is_new_thread())
    }

    /// A reply that is saved for `thread` but not being edited.
    pub(crate) fn parked_reply(&self, thread: &ThreadId) -> Option<&EditingComment> {
        let book = self.book.as_ref()?;
        self.drafts
            .parked
            .get(&(book.review_unit.clone(), thread.clone()))
            .filter(|editing| !editing.draft.is_new_thread())
    }

    /// The open or parked new-thread draft of `file` anchored on any of `rows`.
    pub(crate) fn file_draft_at(
        &self,
        file: &LoadedDocument,
        rows: &RangeInclusive<usize>,
    ) -> Option<ThreadId> {
        self.editing
            .iter()
            .map(|editing| &editing.draft)
            .filter(|draft| draft.is_new_thread())
            .chain(self.parked_file_editors().map(|parked| &parked.draft))
            .filter(|draft| self.matches_path(file, draft.path()))
            .find(|draft| {
                draft_rows(draft, file).is_some_and(|anchored| {
                    anchored.start() <= rows.end() && rows.start() <= anchored.end()
                })
            })
            .map(|draft| draft.thread_id().clone())
    }

    /// Remove focus from the open editor. An editor with no text has nothing to keep.
    pub(crate) fn park_editor(&mut self) {
        if let Some(book) = &self.book
            && let Some(editing) = self.editing.take()
            && (editing.posting.is_some() || !editing.editor.text().trim().is_empty())
        {
            self.drafts.parked.insert(
                (book.review_unit.clone(), editing.draft.thread_id().clone()),
                editing,
            );
        }
    }

    /// Keep an editor from another file out of the file being shown.
    pub(crate) fn park_editor_outside(&mut self, path: &str) {
        if self
            .editing
            .as_ref()
            .is_some_and(|editing| self.paths.resolve(editing.draft.path()) != path)
        {
            self.park_editor();
        }
    }

    pub(in crate::comments) fn activate_editor(&mut self, thread: ThreadId) {
        if self
            .book
            .as_ref()
            .zip(self.editing.as_ref())
            .is_some_and(|(_, editing)| editing.draft.thread_id() == &thread)
        {
            return;
        }
        self.park_editor();
        if let Some(book) = &self.book {
            self.editing = self
                .drafts
                .parked
                .remove(&(book.review_unit.clone(), thread));
        }
    }

    pub(crate) fn enter_conversations(&mut self) {
        self.drafts.file_editor = self
            .book
            .as_ref()
            .zip(self.editing.as_ref())
            .map(|(_, editing)| editing.draft.thread_id().clone());
        self.park_editor();
    }

    pub(crate) fn leave_conversations(&mut self) {
        self.park_editor();
        if let Some(target) = self.drafts.file_editor.take() {
            self.activate_editor(target);
        }
    }

    pub(crate) fn restore_thread_editor(&mut self, id: ThreadId) {
        self.activate_editor(id);
    }
}
