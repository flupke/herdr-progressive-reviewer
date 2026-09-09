//! Unposted editors survive conversation and review navigation.

use std::collections::HashMap;

use review_threads::{ReviewThreads, ThreadId};
use review_types::ReviewUnit;
use ui_events::ThreadPostFinished;

use super::{Comments, EditingComment};

#[derive(Clone, Eq, Hash, PartialEq)]
pub(super) enum DraftTarget {
    Thread(ThreadId),
    File(String),
}

#[derive(Default)]
pub(super) struct Drafts {
    parked: HashMap<(ReviewUnit, DraftTarget), EditingComment>,
    file_editor: Option<DraftTarget>,
}

impl Drafts {
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
    fn target(&self, book: &ReviewThreads) -> DraftTarget {
        self.reply_to
            .as_ref()
            .and_then(|id| book.thread_for_message(id))
            .map_or_else(
                || DraftTarget::File(self.path.clone()),
                |thread| DraftTarget::Thread(thread.id.clone()),
            )
    }
}

impl Comments {
    pub(in crate::comments) fn park_editor(&mut self) {
        if let Some(book) = &self.book
            && let Some(editing) = self.editing.take()
        {
            self.drafts
                .parked
                .insert((book.review_unit.clone(), editing.target(book)), editing);
        }
    }

    pub(in crate::comments) fn activate_editor(&mut self, target: DraftTarget) {
        if self
            .book
            .as_ref()
            .zip(self.editing.as_ref())
            .is_some_and(|(book, editing)| editing.target(book) == target)
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
            .map(|(book, editing)| editing.target(book));
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

    pub(in crate::comments) fn restore_file_editor(&mut self, path: String) {
        self.activate_editor(DraftTarget::File(path));
    }
}
