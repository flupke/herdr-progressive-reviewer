//! Thread state and the diff component's comment commands.

use std::collections::HashMap;
use std::ops::Range;

use comment_editor::{CommentEditor, EditorKeymap};
use review_guide::{DiffRangeAnchor, FrozenHunk, GuideAnchorKind};
use review_threads::{
    Draft, DraftTarget, MessageId, Post, ReviewThread, ReviewThreads, ThreadCommand, ThreadId,
    ThreadSource,
};
use ui_actions::Action;
use ui_events::{ReviewThreadsLoaded, TextPasted, ThreadPostFinished};
use ui_shortcuts::{CommentShortcut, Key};

use crate::{DiffComponent, LoadedDocument, SelectionState};

mod drafts;
use drafts::Drafts;

pub(super) struct Comments {
    pub(super) book: Option<ReviewThreads>,
    pub(super) editing: Option<EditingComment>,
    pub(super) selected: Option<MessageId>,
    pub(super) pending_path: Option<String>,
    mapped: HashMap<ThreadId, Option<FrozenHunk>>,
    pub(super) paths: review_threads::ThreadPaths,
    pub(super) editor_height: u16,
    keymap: EditorKeymap,
    drafts: Drafts,
}

#[derive(Clone)]
pub(super) enum CommentTarget {
    Message(MessageId),
    Reply(MessageId),
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
            Self::Conversation(_) | Self::ConversationButtons(_) => None,
        }
    }
}

impl Default for Comments {
    fn default() -> Self {
        Self {
            book: None,
            editing: None,
            selected: None,
            pending_path: None,
            mapped: HashMap::new(),
            paths: review_threads::ThreadPaths::default(),
            editor_height: 5,
            keymap: EditorKeymap::default(),
            drafts: Drafts::default(),
        }
    }
}

pub(super) struct EditingComment {
    pub(super) posting: Option<MessageId>,
    pub(super) draft: Draft,
    pub(super) editor: CommentEditor,
}

impl Comments {
    pub(super) fn threads(&self) -> impl Iterator<Item = &ReviewThread> {
        self.book.iter().flat_map(ReviewThreads::threads)
    }

    pub(super) fn open_threads(&self) -> impl Iterator<Item = &ReviewThread> {
        self.threads()
            .filter(|thread| thread.resolution == review_threads::Resolution::Open)
    }

    pub(super) fn inline_editor_visible_in(&self, file: &LoadedDocument) -> bool {
        self.editing.as_ref().is_some_and(|editing| {
            self.matches_path(file, editing.draft.path())
                && editing.draft.reply_to.as_ref().is_none_or(|id| {
                    self.open_threads()
                        .any(|thread| thread.messages.iter().any(|message| &message.id == id))
                })
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

    fn prepare_post(&mut self) -> Option<Post> {
        let editing = self.editing.as_mut()?;
        if editing.posting.is_some() {
            return None;
        }
        self.book.as_ref()?;
        editing.draft.text = editing.editor.text();
        let post = editing.draft.post();
        editing.posting = Some(post.message().id.clone());
        Some(post)
    }

    pub(super) fn start_reply(&mut self, id: MessageId) {
        let Some(thread_id) = self
            .book
            .as_ref()
            .and_then(|book| book.thread_for_message(&id))
            .map(|thread| thread.id.clone())
        else {
            return;
        };
        self.activate_editor(DraftTarget::Thread(thread_id));
        if self.editing.is_some() {
            return;
        }
        let Some(thread) = self
            .book
            .as_ref()
            .and_then(|book| book.thread_for_message(&id))
        else {
            return;
        };
        self.editing = Some(EditingComment {
            posting: None,
            draft: Draft::reply(thread, id.clone()),
            editor: CommentEditor::new("", self.keymap),
        });
        self.selected = Some(id);
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
            CommentShortcut::Previous => self.navigate_comment(false),
            CommentShortcut::Next => self.navigate_comment(true),
        }
        Vec::new()
    }

    pub(super) fn comment_key(&mut self, key: Key) -> Vec<Action> {
        if key == Key::ControlEnter {
            return self.finish_comment(EditorAction::Submit);
        }
        if let Some(editing) = &mut self.comments.editing
            && editing.posting.is_none()
        {
            editing.editor.input(key);
            self.comments.keymap = editing.editor.keymap();
        }
        self.save_comment_draft()
    }

    fn save_comment_draft(&mut self) -> Vec<Action> {
        let Some(editing) = &mut self.comments.editing else {
            return Vec::new();
        };
        let Some(book) = &self.comments.book else {
            return Vec::new();
        };
        let text = editing.editor.text();
        if editing.draft.text == text {
            return Vec::new();
        }
        editing.draft.text = text;
        vec![Action::Thread(ThreadCommand::SaveDraft {
            review_unit: book.review_unit.clone(),
            draft: editing.draft.clone(),
        })]
    }

    pub(super) fn finish_comment(&mut self, action: EditorAction) -> Vec<Action> {
        let Some(editing) = &self.comments.editing else {
            return Vec::new();
        };
        if editing.posting.is_some() {
            return Vec::new();
        }
        match action {
            EditorAction::Submit if !editing.editor.text().trim().is_empty() => {
                let Some(post) = self.comments.prepare_post() else {
                    return Vec::new();
                };
                let review_unit = self
                    .comments
                    .book
                    .as_ref()
                    .expect("posting requires a review")
                    .review_unit
                    .clone();
                vec![Action::Thread(review_threads::ThreadCommand::Post {
                    review_unit,
                    post,
                })]
            }
            EditorAction::Submit | EditorAction::Cancel => {
                let discard = self.comments.book.as_ref().map(|book| {
                    Action::Thread(ThreadCommand::DiscardDraft {
                        review_unit: book.review_unit.clone(),
                        target: editing.draft.target.clone(),
                    })
                });
                self.comments.selected.clone_from(&editing.draft.reply_to);
                self.comments.editing = None;
                if !self.conversation.active {
                    self.selection = None;
                }
                self.refresh_comment_documents();
                discard.into_iter().collect()
            }
        }
    }

    pub(super) fn comment_paste(&mut self, input: &TextPasted) -> Vec<Action> {
        if !self.editor_is_visible() {
            return Vec::new();
        }
        if let Some(editing) = &mut self.comments.editing
            && editing.posting.is_none()
        {
            editing.editor.paste(&input.0);
        }
        self.save_comment_draft()
    }

    pub(super) fn add_comment(&mut self) {
        if let Some(path) = &self.selected_path {
            self.comments.restore_file_editor(path);
        }
        if self.comments.editing.is_some() {
            return;
        }
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
        let editing = EditingComment {
            posting: None,
            editor: CommentEditor::new("", self.comments.keymap),
            draft: Draft::start(
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
                        target_kind: GuideAnchorKind::Lines,
                        source_hunk_count: 0,
                        old_content: content.old_content.clone(),
                        new_content: content.new_content.clone(),
                        diff_hash: String::new(),
                    },
                }),
            ),
        };
        self.comments.editing = Some(editing);
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

    fn navigate_comment(&mut self, forward: bool) {
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
        let Some(range) = viewport.comment_range(
            self.comments.selected.as_ref(),
            self.comments.editing.is_some(),
        ) else {
            return;
        };
        let height = usize::from(self.viewport_height);
        let editing = self.comments.editing.is_some();
        if let Some(file) = self.displayed_document_mut() {
            let scroll = &mut file.document.scroll;
            if editing {
                *scroll = (*scroll).max(range.end().saturating_add(1).saturating_sub(height));
            } else if *range.start() < *scroll || *range.end() >= scroll.saturating_add(height) {
                *scroll = range
                    .start()
                    .saturating_sub(1)
                    .min(viewport.visible_row_count().saturating_sub(height));
            }
        }
    }

    pub(super) fn threads_loaded(&mut self, event: &ReviewThreadsLoaded) -> Vec<Action> {
        if self
            .review_checkpoint
            .as_ref()
            .map(|checkpoint| &checkpoint.review_unit)
            != Some(&event.review_unit)
        {
            return Vec::new();
        }
        match &event.result {
            Ok(book) => {
                self.comments.drafts.recover(book, self.comments.keymap);
                self.comments.book = Some(book.clone());
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
            Err(message) => self.events.publish(ui_events::ToastRequested {
                text: format!("Could not load comments: {message}"),
                kind: toasts::ToastKind::Error,
            }),
        }
        self.refresh_comment_documents();
        self.refresh_conversation_context();
        Vec::new()
    }

    fn restore_saved_editor(&mut self) {
        if self.comments.editing.is_none()
            && self.conversation.active
            && let Some(thread) = self.conversation_thread()
        {
            self.comments.restore_thread_editor(thread.id.clone());
        }
        if self.comments.editing.is_none()
            && !self.conversation.active
            && let Some(path) = &self.selected_path
        {
            self.comments.restore_file_editor(path);
        }
    }

    pub(super) fn post_finished(&mut self, event: &ThreadPostFinished) -> Vec<Action> {
        let current = self
            .review_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.review_unit == event.review_unit);
        if current
            && self
                .comments
                .editing
                .as_ref()
                .and_then(|editing| editing.posting.as_ref())
                == Some(&event.message_id)
        {
            if event.result.is_ok() {
                self.comments.editing = None;
                self.comments.selected = Some(event.message_id.clone());
                if !self.conversation.active {
                    self.selection = None;
                }
                self.keep_comment_visible();
            } else if let Some(editing) = &mut self.comments.editing {
                editing.posting = None;
            }
        }

        self.comments.drafts.post_finished(event);
        if let Err(error) = &event.result {
            self.events.publish(ui_events::ToastRequested {
                text: format!("Could not post comment: {error}"),
                kind: toasts::ToastKind::Error,
            });
        }
        Vec::new()
    }

    pub(super) fn refresh_comment_documents(&mut self) {
        let mut previous = std::mem::take(&mut self.documents);
        let (mut orphans, current): (Vec<_>, Vec<_>) =
            previous.drain(..).partition(|file| file.comments_only);
        self.documents = current;
        let mut paths = Vec::new();
        for path in self
            .comments
            .threads()
            .map(ReviewThread::path)
            .chain(self.comments.draft_paths())
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
        }
        Vec::new()
    }
}
