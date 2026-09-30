//! Which draft this viewer edits. The drafts themselves belong to the shared
//! [`Drafts`] of the review; a viewer only focuses one of them at a time and asks
//! the others where they belong.

use std::cell::Ref;
use std::ops::RangeInclusive;

use review_drafts::{DraftId, Drafts, OpenDraft};
use review_threads::{DraftTarget, ThreadId};

use super::Comments;
use crate::LoadedDocument;
use crate::comment_layout::draft_rows;

impl Comments {
    /// The shared drafts, read while this viewer lays out or inspects them.
    pub(crate) fn drafts(&self) -> Ref<'_, Drafts> {
        self.drafts.borrow()
    }

    /// The draft this viewer edits, while it is still a draft.
    pub(crate) fn focused(&self) -> Option<Ref<'_, OpenDraft>> {
        let id = self.focus?;
        Ref::filter_map(self.drafts.borrow(), |drafts| drafts.get(id)).ok()
    }

    pub(crate) fn focused_id(&self) -> Option<DraftId> {
        self.focus
            .filter(|id| self.drafts.borrow().get(*id).is_some())
    }

    /// Drafts of the shown review that this viewer does not edit.
    fn parked<'a>(
        &'a self,
        drafts: &'a Drafts,
    ) -> impl Iterator<Item = (DraftId, &'a OpenDraft)> + 'a {
        self.book
            .iter()
            .flat_map(move |book| drafts.in_review(&book.review_unit))
            .filter(move |(id, _)| Some(*id) != self.focus)
    }

    /// New-thread drafts of the shown review that this viewer does not edit.
    pub(crate) fn parked_file_drafts<'a>(
        &'a self,
        drafts: &'a Drafts,
    ) -> impl Iterator<Item = (DraftId, &'a OpenDraft)> + 'a {
        self.parked(drafts)
            .filter(|(_, open)| open.draft().is_new_thread())
    }

    /// A reply that is saved for `thread` but not being edited here.
    pub(crate) fn parked_reply<'a>(
        &'a self,
        drafts: &'a Drafts,
        thread: &ThreadId,
    ) -> Option<(DraftId, &'a OpenDraft)> {
        self.parked(drafts)
            .find(|(_, open)| !open.draft().is_new_thread() && open.draft().thread_id() == thread)
    }

    pub(super) fn draft_paths(&self) -> Vec<String> {
        let Some(book) = &self.book else {
            return Vec::new();
        };
        self.drafts
            .borrow()
            .in_review(&book.review_unit)
            .filter_map(|(_, open)| match &open.draft().target {
                DraftTarget::File(path) => Some(path.clone()),
                DraftTarget::Thread(_) => None,
            })
            .collect()
    }

    /// The new-thread draft of `file` anchored on any of `rows`, focused or not.
    pub(super) fn file_draft_at(
        &self,
        file: &LoadedDocument,
        rows: &RangeInclusive<usize>,
    ) -> Option<DraftId> {
        let book = self.book.as_ref()?;
        let drafts = self.drafts.borrow();
        drafts
            .in_review(&book.review_unit)
            .filter(|(_, open)| open.draft().is_new_thread())
            .filter(|(_, open)| self.matches_path(file, open.draft().path()))
            .find(|(_, open)| {
                draft_rows(open.draft(), file).is_some_and(|anchored| {
                    anchored.start() <= rows.end() && rows.start() <= anchored.end()
                })
            })
            .map(|(id, _)| id)
    }

    /// Remove focus from the open editor. An editor with no text has nothing to keep.
    pub(crate) fn park_editor(&mut self) {
        if let Some(id) = self.focus.take() {
            self.drafts.borrow_mut().park(id);
        }
    }

    /// Keep an editor from another file out of the file being shown.
    pub(crate) fn park_editor_outside(&mut self, path: &str) {
        if self
            .focused()
            .is_some_and(|open| self.paths.resolve(open.draft().path()) != path)
        {
            self.park_editor();
        }
    }

    pub(super) fn activate(&mut self, id: DraftId) {
        if self.focus == Some(id) {
            return;
        }
        self.park_editor();
        let unit = self.book.as_ref().map(|book| &book.review_unit);
        let shown = self
            .drafts
            .borrow()
            .get(id)
            .is_some_and(|open| Some(open.unit()) == unit);
        self.focus = shown.then_some(id);
    }

    pub(crate) fn enter_conversations(&mut self) {
        self.file_focus = self.focused_id();
        self.park_editor();
    }

    pub(crate) fn leave_conversations(&mut self) {
        self.park_editor();
        if let Some(id) = self.file_focus.take() {
            self.activate(id);
        }
    }

    /// Edit the draft saved for `thread`, if there is one.
    pub(crate) fn restore_thread_editor(&mut self, thread: &ThreadId) {
        let id = self
            .book
            .as_ref()
            .and_then(|book| self.drafts.borrow().for_thread(&book.review_unit, thread));
        match id {
            Some(id) => self.activate(id),
            None => self.park_editor(),
        }
    }
}
