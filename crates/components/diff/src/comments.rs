//! Thread state and the diff component's comment commands.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use comment_editor::KeymapSetting;
use review_drafts::{DraftId, Drafts, Submission};
use review_source::{AnchorKind, DiffRangeAnchor, FrozenHunk};
use review_threads::{
    Draft, MessageId, ReviewThread, ReviewThreads, ThreadCommand, ThreadId, ThreadPaths,
    ThreadSource,
};
use ui_actions::Action;
use ui_events::{ReviewThreadsLoaded, TextPasted, ThreadPostFinished};
use ui_shortcuts::{CommentShortcut, Key};

use crate::{DiffComponent, LoadedDocument, SelectionState};

mod focus;

pub(super) struct Comments {
    book: Option<ReviewThreads>,
    /// Shared with every nested viewer, so each draft has one owner.
    drafts: Rc<RefCell<Drafts>>,
    focus: Option<DraftId>,
    /// The file editor to restore when the conversation view closes.
    file_focus: Option<DraftId>,
    selected: Option<MessageId>,
    pending_path: Option<String>,
    mapped: HashMap<ThreadId, Option<FrozenHunk>>,
    paths: ThreadPaths,
    editor_height: u16,
}

#[derive(Clone)]
pub(super) enum CommentTarget {
    Message(MessageId),
    Reply(MessageId),
    Draft(DraftId),
    Conversation(crate::conversation::ConversationAction),
    ConversationButtons(Vec<ConversationButton>),
}

#[derive(Clone, Copy)]
pub(super) enum EditorAction {
    Submit,
    Cancel,
}

#[derive(Clone)]
pub(super) struct ConversationButton {
    pub(super) action: crate::conversation::ConversationAction,
    pub(super) columns: Range<usize>,
}

impl CommentTarget {
    pub(super) fn id(&self) -> Option<&MessageId> {
        match self {
            Self::Message(id) | Self::Reply(id) => Some(id),
            Self::Draft(_) | Self::Conversation(_) | Self::ConversationButtons(_) => None,
        }
    }
}

impl Default for Comments {
    fn default() -> Self {
        Self {
            book: None,
            drafts: Rc::default(),
            focus: None,
            file_focus: None,
            selected: None,
            pending_path: None,
            mapped: HashMap::new(),
            paths: ThreadPaths::default(),
            editor_height: 5,
        }
    }
}

impl Comments {
    /// Let a nested viewer edit the same drafts as this one.
    pub(super) fn share_drafts(&mut self, other: &Self) {
        self.drafts = Rc::clone(&other.drafts);
    }

    pub(super) fn use_keymap(&mut self, keymap: KeymapSetting) {
        self.drafts.borrow_mut().use_keymap(keymap);
    }

    pub(super) fn use_paths(&mut self, paths: ThreadPaths) {
        self.paths = paths;
    }

    /// Size editors for a viewport of `height` rows.
    pub(super) fn fit_editors(&mut self, height: u16) {
        self.editor_height = height.saturating_sub(6).clamp(1, 5);
    }

    pub(super) fn editor_height(&self) -> u16 {
        self.editor_height
    }

    pub(super) fn book(&self) -> Option<&ReviewThreads> {
        self.book.as_ref()
    }

    /// Whether `path` finished loading after comment navigation asked for it.
    pub(super) fn take_pending_path(&mut self, path: &str) -> bool {
        let pending = self.pending_path.as_deref() == Some(path);
        if pending {
            self.pending_path = None;
        }
        pending
    }

    pub(super) fn inherit_book(&mut self, book: &ReviewThreads) {
        self.recover(book);
        self.book = Some(book.clone());
    }

    /// Reopen the drafts `book` saved that no viewer holds yet.
    fn recover(&self, book: &ReviewThreads) {
        self.drafts.borrow_mut().recover(book);
    }

    pub(super) fn threads(&self) -> impl Iterator<Item = &ReviewThread> {
        self.book.iter().flat_map(ReviewThreads::threads)
    }

    pub(super) fn open_threads(&self) -> impl Iterator<Item = &ReviewThread> {
        self.threads()
            .filter(|thread| thread.resolution == review_threads::Resolution::Open)
    }

    pub(super) fn inline_editor_visible_in(&self, file: &LoadedDocument) -> bool {
        self.focused().is_some_and(|open| {
            let draft = open.draft();
            self.matches_path(file, draft.path())
                && draft.reply_to.as_ref().is_none_or(|id| {
                    self.open_threads()
                        .any(|thread| thread.messages.iter().any(|message| &message.id == id))
                })
        })
    }

    /// Whether this viewer edits a reply to one of `thread`'s messages.
    pub(super) fn replies_to(&self, thread: &ReviewThread) -> bool {
        self.focused().is_some_and(|open| {
            thread
                .messages
                .iter()
                .any(|message| open.draft().reply_to.as_ref() == Some(&message.id))
        })
    }

    pub(super) fn refresh_anchors(&mut self, documents: &[LoadedDocument]) {
        self.mapped.clear();
        let Some(book) = &self.book else { return };
        for thread in book.threads() {
            let mapped = documents
                .iter()
                .find(|file| self.matches_path(file, thread.path()))
                .filter(|file| !file.comments_only)
                .and_then(|file| file.content.as_ref())
                .and_then(|content| {
                    thread.anchor.map_lines(
                        content.old_content.as_deref(),
                        content.new_content.as_deref(),
                    )
                });
            self.mapped.insert(thread.id.clone(), mapped);
        }
    }

    pub(super) fn matches_path(&self, file: &LoadedDocument, path: &str) -> bool {
        self.paths.resolve(path) == file.path
    }

    pub(super) fn mapped(&self, thread: &ThreadId) -> Option<&FrozenHunk> {
        self.mapped.get(thread).and_then(Option::as_ref)
    }

    pub(super) fn start_reply(&mut self, id: MessageId) {
        let Some(book) = &self.book else { return };
        let Some(thread) = book.thread_for_message(&id) else {
            return;
        };
        let mut drafts = self.drafts.borrow_mut();
        let existing = drafts.for_thread(&book.review_unit, &thread.id);
        let started = drafts.start_reply(book.review_unit.clone(), thread, id.clone());
        drop(drafts);
        if let Some(draft) = existing.or(started) {
            self.activate(draft);
        }
        if started.is_some() {
            self.selected = Some(id);
        }
    }

    /// Change the focused draft's text, returning the save it needs.
    fn edit(
        &mut self,
        edit: impl FnOnce(&mut Drafts, DraftId) -> Option<ThreadCommand>,
    ) -> Option<ThreadCommand> {
        let id = self.focused_id()?;
        edit(&mut self.drafts.borrow_mut(), id)
    }

    /// Submit or cancel the focused draft. A draft that is no longer composed
    /// returns focus to the message it answered.
    fn finish(&mut self, action: EditorAction) -> Option<Submission> {
        let id = self.focused_id()?;
        let reply_to = self
            .focused()
            .and_then(|open| open.draft().reply_to.clone());
        let submission = match action {
            EditorAction::Submit => self.drafts.borrow_mut().submit(id)?,
            EditorAction::Cancel => Submission::Cancelled(self.drafts.borrow_mut().cancel(id)?),
        };
        if let Submission::Cancelled(_) = submission {
            self.focus = None;
            self.selected = reply_to;
        }
        Some(submission)
    }

    /// Adopt a loaded `book`: settle the drafts it shows posted and return the
    /// saves of drafts renewed meanwhile.
    fn accept_book(&mut self, book: &ReviewThreads) -> Vec<ThreadCommand> {
        let renewed = self.drafts.borrow_mut().reconcile(book);
        self.book = Some(book.clone());
        renewed
    }

    fn post_finished(&self, event: &ThreadPostFinished) {
        let mut drafts = self.drafts.borrow_mut();
        if event.result.is_ok() {
            drafts.post_succeeded(&event.review_unit, &event.message_id);
        } else {
            drafts.post_failed(&event.review_unit, &event.message_id);
        }
    }

    fn start_thread(&mut self, draft: Draft) {
        let Some(book) = &self.book else { return };
        let id = self
            .drafts
            .borrow_mut()
            .start(book.review_unit.clone(), draft);
        self.activate(id);
    }

    /// Once the focused draft has been posted, select its message instead.
    fn release_posted(&mut self) -> bool {
        let posted = self
            .focus
            .and_then(|id| self.drafts.borrow().posted_as(id).cloned());
        let released = posted.is_some();
        if released {
            self.focus = None;
            self.selected = posted;
        }
        released
    }
}

impl DiffComponent {
    pub(super) fn prepare_review_switch(&mut self, same_review_unit: bool) {
        if !same_review_unit {
            self.comments.park_editor();
            self.close_peek();
            self.conversation.reset_review();
            self.comments.book = None;
            self.comments.selected = None;
            self.comments.pending_path = None;
            self.comments.mapped.clear();
        }
    }

    pub(super) fn comment_shortcut(&mut self, command: CommentShortcut) -> Vec<Action> {
        match command {
            CommentShortcut::Add => self.add_comment(),
            CommentShortcut::Reply => {
                if let Some(id) = self.current_comment() {
                    self.start_comment(id);
                }
            }
        }
        Vec::new()
    }

    pub(super) fn comment_key(&mut self, key: Key) -> Vec<Action> {
        if key == Key::ControlEnter {
            return self.finish_comment(EditorAction::Submit);
        }
        let save = self.comments.edit(|drafts, id| drafts.input(id, key));
        save.map(Action::Thread).into_iter().collect()
    }

    pub(super) fn finish_comment(&mut self, action: EditorAction) -> Vec<Action> {
        match self.comments.finish(action) {
            Some(Submission::Posting(post)) => vec![Action::Thread(post)],
            Some(Submission::Cancelled(discard)) => {
                if !self.conversation.active {
                    self.selection = None;
                }
                self.refresh_comment_documents();
                vec![Action::Thread(discard)]
            }
            None => Vec::new(),
        }
    }

    pub(super) fn comment_paste(&mut self, input: &TextPasted) -> Vec<Action> {
        if !self.editor_is_visible() {
            return Vec::new();
        }
        let save = self.comments.edit(|drafts, id| drafts.paste(id, &input.0));
        save.map(Action::Thread).into_iter().collect()
    }

    pub(super) fn add_comment(&mut self) {
        if self.comments.book.is_none() {
            self.events.publish(ui_events::ToastRequested {
                text: "Comments are still loading".into(),
                kind: toasts::ToastKind::Error,
            });
            return;
        }
        let Some(file) = self.selected_document() else {
            return;
        };
        let Some(content) = &file.content else { return };
        let range = self.selection.map_or(
            file.document.cursor..=file.document.cursor,
            SelectionState::range,
        );
        if let Some(thread) = self.comments.file_draft_at(file, &range) {
            self.open_draft(thread);
            return;
        }
        let mut old = None;
        let mut new = None;
        for row in range.clone() {
            match file.document.diff.presentation_location(row) {
                Some(ui_events::PresentationLocation::Context { old_line, new_line }) => {
                    extend_lines(&mut old, old_line);
                    extend_lines(&mut new, new_line);
                }
                Some(ui_events::PresentationLocation::OldLine(line)) => {
                    extend_lines(&mut old, line);
                }
                Some(ui_events::PresentationLocation::NewLine(line)) => {
                    extend_lines(&mut new, line);
                }
                _ => {}
            }
        }
        if old.is_none() && new.is_none() {
            return;
        }
        let excerpt = file.document.diff.comment_excerpt(range);
        let draft = Draft::start(
            file.path.clone(),
            std::sync::Arc::new(ThreadSource {
                excerpt,
                anchor: DiffRangeAnchor {
                    source_checkpoint: content.review_checkpoint.checkpoint.clone(),
                    old_path: file
                        .old_path
                        .clone()
                        .or_else(|| old.as_ref().map(|_| file.path.clone())),
                    new_path: file
                        .new_path
                        .clone()
                        .or_else(|| new.as_ref().map(|_| file.path.clone())),
                    old_lines: old,
                    new_lines: new,
                    target_kind: AnchorKind::Lines,
                    source_hunk_count: 0,
                    old_content: content.old_content.clone(),
                    new_content: content.new_content.clone(),
                    diff_hash: String::new(),
                },
            }),
        );
        self.comments.start_thread(draft);
        self.selection = None;
        self.keep_comment_visible();
    }

    /// Buttons of an unfocused editor act on their own draft.
    pub(super) fn finish_draft(&mut self, draft: DraftId, action: EditorAction) -> Vec<Action> {
        self.comments.activate(draft);
        self.finish_comment(action)
    }

    fn open_draft(&mut self, draft: DraftId) {
        self.comments.activate(draft);
        self.selection = None;
        self.keep_comment_visible();
    }

    fn start_comment(&mut self, id: MessageId) {
        self.comments.start_reply(id);
        if !self.conversation.active {
            self.selection = None;
        }
        self.keep_comment_visible();
    }

    fn open_comment(&mut self, id: MessageId) {
        self.comments.selected = Some(id);
        if !self.conversation.active {
            self.selection = None;
            self.keep_comment_visible();
        }
    }

    fn current_comment(&self) -> Option<MessageId> {
        if self.conversation.active {
            return self
                .conversation_thread()
                .and_then(|thread| thread.messages.last())
                .map(|message| message.id.clone());
        }
        let file = self.selected_document()?;
        let threads = self
            .comments
            .open_threads()
            .filter(|thread| self.comments.matches_path(file, thread.path()))
            .collect::<Vec<_>>();
        if let Some(id) = &self.comments.selected
            && threads
                .iter()
                .any(|thread| thread.messages.iter().any(|comment| &comment.id == id))
        {
            return Some(id.clone());
        }
        threads
            .iter()
            .filter(|thread| self.comments.thread_row(thread, file).0 == file.document.cursor)
            .flat_map(|thread| &thread.messages)
            .last()
            .map(|comment| comment.id.clone())
    }

    pub(super) fn navigate_comment(&mut self, forward: bool) {
        let comments = self
            .comments
            .open_threads()
            .flat_map(|thread| {
                thread
                    .messages
                    .iter()
                    .map(move |comment| (thread.path(), &comment.id))
            })
            .collect::<Vec<_>>();
        if comments.is_empty() {
            return;
        }
        let current = comments
            .iter()
            .position(|(_, id)| Some(*id) == self.comments.selected.as_ref());
        let index = match (current, forward) {
            (Some(index), true) => (index + 1) % comments.len(),
            (Some(index), false) => (index + comments.len() - 1) % comments.len(),
            (None, true) => 0,
            (None, false) => comments.len() - 1,
        };
        let (path, id) = comments[index];
        let path = self
            .documents
            .iter()
            .find(|file| self.comments.matches_path(file, path))
            .map_or_else(|| path.to_owned(), |file| file.path.clone());
        self.comments.selected = Some(id.clone());
        self.selected_path = Some(path.clone());
        self.comments.pending_path = Some(path.clone());
        self.events
            .publish(ui_events::FileSelectionRequested { path });
        self.keep_comment_visible();
    }

    pub(super) fn keep_comment_visible(&mut self) {
        if self.conversation.active {
            self.keep_conversation_composer_visible();
            return;
        }
        let Some(viewport) = self.displayed_viewport() else {
            return;
        };
        let editing = self.comments.focused_id().is_some();
        let Some(range) = viewport.comment_range(self.comments.selected.as_ref(), editing) else {
            return;
        };
        let height = usize::from(self.viewport_height);
        if let Some(file) = self.displayed_document_mut() {
            let scroll = &mut file.document.scroll;
            if editing {
                let bottom = range.end().saturating_add(1).saturating_sub(height);
                // Show the whole editor when it fits, otherwise as much of it as possible.
                *scroll = if bottom <= *range.start() {
                    (*scroll).clamp(bottom, *range.start())
                } else {
                    bottom
                };
            } else if *range.start() < *scroll || *range.end() >= scroll.saturating_add(height) {
                *scroll = range
                    .start()
                    .saturating_sub(1)
                    .min(viewport.visible_row_count().saturating_sub(height));
            }
        }
    }

    pub(super) fn threads_loaded(&mut self, event: &ReviewThreadsLoaded) -> Vec<Action> {
        let actions = self.update_loaded_threads(event);
        if let Err(message) = &event.result {
            self.events.publish(ui_events::ToastRequested {
                text: format!("Could not load comments: {message}"),
                kind: toasts::ToastKind::Error,
            });
        }
        actions
    }

    fn update_loaded_threads(&mut self, event: &ReviewThreadsLoaded) -> Vec<Action> {
        let mut actions: Vec<_> = self
            .retained_viewers_mut()
            .flat_map(|viewer| viewer.update_loaded_threads(event))
            .collect();
        if self
            .review_checkpoint
            .as_ref()
            .map(|checkpoint| &checkpoint.review_unit)
            != Some(&event.review_unit)
        {
            return actions;
        }
        let mut visible_anchor = None;
        if let Ok(book) = &event.result {
            self.comments.recover(book);
            // Another viewer may already have settled the shared draft this one edits.
            let editing = self.comments.focus.is_some();
            let height = usize::from(self.viewport_height);
            visible_anchor = (editing && !self.conversation.active)
                .then(|| {
                    let file = self.displayed_document()?;
                    self.displayed_viewport()?
                        .visible_anchor(file.document.scroll, height)
                })
                .flatten();
            let renewed = self.comments.accept_book(book);
            actions.extend(renewed.into_iter().map(Action::Thread));
            if self.comments.release_posted() {
                self.selection = None;
            } else {
                visible_anchor = None;
            }
            self.restore_saved_editor();
            if self
                .comments
                .selected
                .as_ref()
                .is_some_and(|id| book.message(id).is_none())
            {
                self.comments.selected = None;
            }
        }
        self.refresh_comment_documents();
        // The posted thread is placed in the diff only after its anchor is remapped.
        let scroll = visible_anchor.and_then(|anchor| {
            self.displayed_viewport()?
                .scroll_for_anchor(&anchor, usize::from(self.viewport_height))
        });
        if let Some(scroll) = scroll
            && let Some(file) = self.displayed_document_mut()
        {
            file.document.scroll = scroll;
        }
        self.refresh_conversation_context();
        actions
    }

    fn restore_saved_editor(&mut self) {
        if self.comments.focused_id().is_none()
            && self.conversation.active
            && let Some(thread) = self.conversation_thread().map(|thread| thread.id.clone())
        {
            self.comments.restore_thread_editor(&thread);
        }
    }

    pub(super) fn post_finished(&mut self, event: &ThreadPostFinished) -> Vec<Action> {
        self.update_finished_post(event);
        if let Err(error) = &event.result {
            self.events.publish(ui_events::ToastRequested {
                text: format!("Could not post comment: {error}"),
                kind: toasts::ToastKind::Error,
            });
        }
        Vec::new()
    }

    fn update_finished_post(&mut self, event: &ThreadPostFinished) {
        for viewer in self.retained_viewers_mut() {
            viewer.update_finished_post(event);
        }
        self.comments.post_finished(event);
        let current = self
            .review_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.review_unit == event.review_unit);
        if current && self.comments.release_posted() {
            if !self.conversation.active {
                self.selection = None;
            }
            self.keep_comment_visible();
        }
    }

    pub(super) fn refresh_comment_documents(&mut self) {
        let mut previous = std::mem::take(&mut self.documents);
        let (mut orphans, current): (Vec<_>, Vec<_>) =
            previous.drain(..).partition(|file| file.comments_only);
        self.documents = current;
        let mut paths = Vec::new();
        let draft_paths = self.comments.draft_paths();
        for path in self
            .comments
            .threads()
            .map(ReviewThread::path)
            .chain(draft_paths.iter().map(String::as_str))
        {
            if self
                .documents
                .iter()
                .any(|file| self.comments.matches_path(file, path))
            {
                continue;
            }
            let mut file = orphans
                .iter()
                .position(|file| file.path == path)
                .map_or_else(
                    || LoadedDocument::new(path),
                    |index| orphans.swap_remove(index),
                );
            file.comments_only = true;
            paths.push(file.path.clone());
            self.documents.push(file);
        }
        self.comments.refresh_anchors(&self.documents);
        self.publish_thread_contexts();
        self.events.publish(ui_events::ThreadFilesChanged { paths });
    }
}

fn extend_lines(range: &mut Option<Range<u32>>, line: u32) {
    if let Some(range) = range {
        range.start = range.start.min(line);
        range.end = range.end.max(line.saturating_add(1));
    } else {
        *range = Some(line..line.saturating_add(1));
    }
}

impl DiffComponent {
    pub(super) fn comment_pointer_input(
        &mut self,
        input: ui_events::PointerInput,
    ) -> Option<Vec<Action>> {
        let position = input.position?;
        let row = usize::from(position.component_row.checked_sub(1)?);
        let target = {
            let cached = self.rendered_pointer_viewport.borrow();
            let viewport = cached.as_ref()?;
            if !viewport.is_comment(row) {
                return None;
            }
            viewport.comment_at(row).cloned()
        };
        if matches!(input.kind, ui_events::PointerInputKind::Click)
            && let Some(target) = target
        {
            let column = usize::from(position.component_column.saturating_sub(1));
            return Some(self.activate_comment_target(target, column));
        }
        if let ui_events::PointerInputKind::Scroll(delta) = input.kind {
            self.scroll(delta);
        }
        Some(Vec::new())
    }

    pub(super) fn activate_comment_target(
        &mut self,
        target: CommentTarget,
        column: usize,
    ) -> Vec<Action> {
        match target {
            CommentTarget::Conversation(action) => return self.conversation_action(action),
            CommentTarget::ConversationButtons(buttons) => {
                if let Some(button) = buttons
                    .into_iter()
                    .find(|button| button.columns.contains(&column))
                {
                    return self.conversation_action(button.action);
                }
            }
            CommentTarget::Message(id) => self.open_comment(id),
            CommentTarget::Reply(id) => self.start_comment(id),
            CommentTarget::Draft(thread) => self.open_draft(thread),
        }
        Vec::new()
    }
}
