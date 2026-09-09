//! Unposted editors survive conversation and review navigation.

use std::collections::{HashMap, HashSet};

use comment_editor::{CommentEditor, EditorKeymap};
use review_threads::{DraftTarget, ReviewThreads, ThreadId};
use review_types::ReviewUnit;
use ui_events::ThreadPostFinished;

use super::{Comments, EditingComment};

#[derive(Default)]
pub(super) struct Drafts {
    parked: HashMap<(ReviewUnit, DraftTarget), EditingComment>,
    file_editor: Option<DraftTarget>,
    recovered: HashSet<ReviewUnit>,
}

impl Drafts {
    pub(super) fn recover(&mut self, book: &ReviewThreads, keymap: EditorKeymap) {
        if !self.recovered.insert(book.review_unit.clone()) {
            return;
        }
        for draft in book.drafts() {
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

impl Comments {
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
