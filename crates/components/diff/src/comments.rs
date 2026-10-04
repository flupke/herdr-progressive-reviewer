//! Thread state and the diff component's comment commands.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use review_drafts::{DraftId, Drafts, Submission};
use review_source::{AnchorKind, DiffRangeAnchor, FrozenHunk};
use review_thread_projection::{SharedThreadProjection, ThreadPlacement};
use review_threads::{
    Draft, MessageId, ReviewThread, ReviewThreads, ThreadCommand, ThreadId, ThreadPaths,
    ThreadSource,
};
use ui_actions::Action;
use ui_events::{ReviewThreadsLoaded, TextPasted, ThreadPostFinished};
use ui_shortcuts::{CommentShortcut, Key};

use crate::conversation::ConversationAction;
use crate::{LoadedDocument, SelectionState, SourceViewer};

mod focus;

pub(super) struct Comments {
    book: Option<ReviewThreads>,
    /// Shared with every viewer of the pane, so each draft has one owner.
    drafts: Rc<RefCell<Drafts>>,
    focus: Option<DraftId>,
    site: EditorSite,
    selected: Option<MessageId>,
    pending_path: Option<String>,
    mapped: HashMap<ThreadId, Option<FrozenHunk>>,
    /// Threads whose current source could not be read.
    unavailable: HashSet<ThreadId>,
    files: ThreadFiles,
    editor_height: u16,
}

/// The files a viewer matches threads to.
enum ThreadFiles {
    /// The reviewed files, associated with threads by the shared projection.
    Review(SharedThreadProjection),
    /// The files of a saved Explore comparison.
    Comparison(ThreadPaths),
}

/// Where the pane shows the editor of the focused draft.
enum EditorSite {
    /// In the viewer's document.
    File,
    /// In the pane's conversation view, which edits a reply. The file editor
    /// returns when the conversation closes.
    Conversation { file_focus: Option<DraftId> },
}

#[derive(Clone)]
pub(super) enum CommentTarget {
    Message(MessageId),
    Reply(MessageId),
    Draft(DraftId),
    Conversation(ConversationAction),
    ConversationButtons(Vec<ConversationButton>),
}

#[derive(Clone, Copy)]
pub(super) enum EditorAction {
    Submit,
    Cancel,
}

#[derive(Clone)]
pub(super) struct ConversationButton {
    pub(super) action: ConversationAction,
    pub(super) columns: Range<usize>,
}

impl ConversationButton {
    /// The action of the button under `column`.
    pub(super) fn at(buttons: Vec<Self>, column: usize) -> Option<ConversationAction> {
        buttons
            .into_iter()
            .find(|button| button.columns.contains(&column))
            .map(|button| button.action)
    }
}

impl CommentTarget {
    pub(super) fn id(&self) -> Option<&MessageId> {
        match self {
            Self::Message(id) | Self::Reply(id) => Some(id),
            Self::Draft(_) | Self::Conversation(_) | Self::ConversationButtons(_) => None,
        }
    }
}

impl Comments {
    /// Comments editing the pane's shared `drafts`.
    pub(super) fn new(drafts: Rc<RefCell<Drafts>>, review: SharedThreadProjection) -> Self {
        Self {
            book: None,
            drafts,
            focus: None,
            site: EditorSite::File,
            selected: None,
            pending_path: None,
            mapped: HashMap::new(),
            unavailable: HashSet::new(),
            files: ThreadFiles::Review(review),
            editor_height: 5,
        }
    }

    /// Whether the focus edits a reply in the pane's conversation view.
    pub(super) fn in_conversation(&self) -> bool {
        matches!(self.site, EditorSite::Conversation { .. })
    }

    pub(super) fn is_unavailable(&self, thread: &ThreadId) -> bool {
        self.unavailable.contains(thread)
    }

    /// Record whether the current source of `thread` could be read.
    pub(super) fn set_unavailable(&mut self, thread: ThreadId, unavailable: bool) {
        if unavailable {
            self.unavailable.insert(thread);
        } else {
            self.unavailable.remove(&thread);
        }
    }

    pub(super) fn forget_unavailable(&mut self) {
        self.unavailable.clear();
    }

    /// Whether a thread saved on `path` belongs to the file at `current` now.
    pub(super) fn is_current_path(&self, path: &str, current: &str) -> bool {
        match &self.files {
            ThreadFiles::Review(projection) => projection.read().current_path(path) == current,
            ThreadFiles::Comparison(paths) => paths.resolve(path) == current,
        }
    }

    /// Match threads to the files of a saved comparison instead of the review.
    pub(super) fn use_comparison(&mut self, paths: ThreadPaths) {
        self.files = ThreadFiles::Comparison(paths);
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

    /// Drafts are shared, so the viewer that loaded `book` already recovered its drafts.
    pub(super) fn inherit_book(&mut self, book: &ReviewThreads) {
        self.book = Some(book.clone());
    }

    /// Reopen the drafts saved for `unit` that no viewer holds yet.
    fn recover(&self, unit: &review_types::ReviewUnit, saved: &review_threads::SavedDrafts) {
        self.drafts.borrow_mut().recover(unit, saved);
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
                .find(|file| self.shows_thread(file, thread))
                .filter(|file| !file.comments_only)
                .and_then(|file| file.content.as_ref())
                .zip(thread.code())
                .and_then(|(content, code)| {
                    code.anchor.map_lines(
                        content.old_content.as_deref(),
                        content.new_content.as_deref(),
                    )
                });
            self.mapped.insert(thread.id.clone(), mapped);
        }
    }

    /// Whether `file` holds the code `thread` discusses; a round conversation is on no file.
    pub(super) fn shows_thread(&self, file: &LoadedDocument, thread: &ReviewThread) -> bool {
        thread
            .path()
            .is_some_and(|path| self.matches_path(file, path))
    }

    pub(super) fn matches_path(&self, file: &LoadedDocument, path: &str) -> bool {
        self.is_current_path(path, &file.path)
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

impl SourceViewer {
    /// Whether this viewer shows the editor of its focused draft.
    pub(super) fn editor_is_visible(&self) -> bool {
        !self.comments.in_conversation()
            && self
                .selected_document()
                .is_some_and(|file| self.comments.inline_editor_visible_in(file))
    }

    pub(super) fn prepare_review_switch(&mut self, same_review_unit: bool) {
        if !same_review_unit {
            self.comments.park_editor();
            self.comments.unavailable.clear();
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
                if !self.comments.in_conversation() {
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
        self.paste_into_draft(input)
    }

    /// Paste into the focused draft, wherever the pane shows its editor.
    pub(super) fn paste_into_draft(&mut self, input: &TextPasted) -> Vec<Action> {
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
        let cursor = file.document.position().cursor();
        let range = self
            .selection
            .map_or(cursor..=cursor, SelectionState::range);
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

    pub(super) fn open_draft(&mut self, draft: DraftId) {
        self.comments.activate(draft);
        self.selection = None;
        self.keep_comment_visible();
    }

    pub(super) fn start_comment(&mut self, id: MessageId) {
        self.comments.start_reply(id);
        if !self.comments.in_conversation() {
            self.selection = None;
        }
        self.keep_comment_visible();
    }

    pub(super) fn open_comment(&mut self, id: MessageId) {
        self.comments.selected = Some(id);
        if !self.comments.in_conversation() {
            self.selection = None;
            self.keep_comment_visible();
        }
    }

    fn current_comment(&self) -> Option<MessageId> {
        let file = self.selected_document()?;
        let threads = self
            .comments
            .open_threads()
            .filter(|thread| self.comments.shows_thread(file, thread))
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
            .filter(|thread| {
                self.comments.thread_row(thread, file).0 == file.document.position().cursor()
            })
            .flat_map(|thread| &thread.messages)
            .last()
            .map(|comment| comment.id.clone())
    }

    pub(super) fn navigate_comment(&mut self, forward: bool) {
        let comments = self
            .comments
            .open_threads()
            .filter_map(|thread| Some((thread.path()?, thread)))
            .flat_map(|(path, thread)| {
                thread
                    .messages
                    .iter()
                    .map(move |comment| (path, &comment.id))
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

    /// Scroll the open editor or the selected comment into view. The pane
    /// keeps an editor shown in its conversation visible itself.
    pub(super) fn keep_comment_visible(&mut self) {
        if self.comments.in_conversation() {
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
            let (position, rows) = file.document.on_screen(&viewport);
            position.reveal_comment(range, editing, &rows, height);
        }
    }

    /// Show the threads and drafts loaded for this viewer's review.
    pub(super) fn threads_loaded(&mut self, event: &ReviewThreadsLoaded) -> Vec<Action> {
        let mut actions = Vec::new();
        if !self.role.shows_comments()
            || self
                .review_checkpoint
                .as_ref()
                .map(|checkpoint| &checkpoint.review_unit)
                != Some(&event.review_unit)
        {
            return actions;
        }
        let mut visible_anchor = None;
        if let Ok(book) = &event.result {
            self.comments.recover(&event.review_unit, &event.drafts);
            // Another viewer may already have settled the shared draft this one edits.
            let editing = self.comments.focus.is_some();
            let height = usize::from(self.viewport_height);
            visible_anchor = (editing && !self.comments.in_conversation())
                .then(|| {
                    let file = self.displayed_document()?;
                    self.displayed_viewport()?
                        .visible_anchor(file.document.position().scroll(), height)
                })
                .flatten();
            let renewed = self.comments.accept_book(book);
            actions.extend(renewed.into_iter().map(Action::Thread));
            if self.comments.release_posted() {
                self.selection = None;
            } else {
                visible_anchor = None;
            }
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
        let pinned = visible_anchor.and_then(|anchor| {
            let viewport = self.displayed_viewport()?;
            let row = viewport.anchor_row(&anchor)?;
            Some((viewport, row, anchor.screen_row()))
        });
        let height = usize::from(self.viewport_height);
        if let Some((viewport, row, screen_row)) = pinned
            && let Some(file) = self.displayed_document_mut()
        {
            let (position, rows) = file.document.on_screen(&viewport);
            position.pin(row, screen_row, &rows, height);
        }
        actions
    }

    /// Settle the draft whose post finished. Returns whether this viewer's
    /// editor held that draft and now selects the posted message.
    pub(super) fn post_finished(&mut self, event: &ThreadPostFinished) -> bool {
        if !self.role.shows_comments() {
            return false;
        }
        self.comments.post_finished(event);
        let current = self
            .review_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.review_unit == event.review_unit);
        let released = current && self.comments.release_posted();
        if released {
            if !self.comments.in_conversation() {
                self.selection = None;
            }
            self.keep_comment_visible();
        }
        released
    }

    /// Where this viewer's loaded code places `thread`.
    fn thread_placement(&self, thread: &ReviewThread) -> ThreadPlacement {
        if self.comments.is_unavailable(&thread.id) {
            return ThreadPlacement::Unavailable;
        }
        let Some(file) = self
            .documents
            .iter()
            .find(|file| !file.comments_only && self.comments.shows_thread(file, thread))
        else {
            return ThreadPlacement::OutsideDiff;
        };
        if file.content.is_none() {
            return ThreadPlacement::Original;
        }
        if self.comments.mapped(&thread.id).is_none() {
            return ThreadPlacement::Earlier;
        }
        if file.document.diff.is_empty() || self.comments.thread_row(thread, file).1 {
            return ThreadPlacement::Hidden;
        }
        ThreadPlacement::Current
    }

    /// Report where this viewer's loaded code places each thread.
    pub(super) fn place_threads(&self) {
        if let Some(book) = self.comments.book() {
            self.thread_projection.place(
                &book.review_unit,
                book.threads()
                    .iter()
                    .map(|thread| (thread.id.clone(), self.thread_placement(thread)))
                    .collect(),
            );
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
            .filter_map(ReviewThread::path)
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
        self.place_threads();
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

impl SourceViewer {
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
            CommentTarget::Conversation(action) => return self.comment_action(action),
            CommentTarget::ConversationButtons(buttons) => {
                if let Some(action) = ConversationButton::at(buttons, column) {
                    return self.comment_action(action);
                }
            }
            CommentTarget::Message(id) => self.open_comment(id),
            CommentTarget::Reply(id) => self.start_comment(id),
            CommentTarget::Draft(thread) => self.open_draft(thread),
        }
        Vec::new()
    }

    /// Run a button of a thread or editor shown in the diff.
    pub(super) fn comment_action(&mut self, action: ConversationAction) -> Vec<Action> {
        match action {
            ConversationAction::Resolve(id) => self.resolve_thread(&id),
            ConversationAction::Retry(id) => self.retry_thread(id),
            ConversationAction::Editor(action, draft) => self.finish_draft(draft, action),
            // Only the pane's conversation view shows these buttons.
            ConversationAction::Back
            | ConversationAction::Reply
            | ConversationAction::Peek
            | ConversationAction::OpenFile
            | ConversationAction::ClosePeek
            | ConversationAction::Read => Vec::new(),
        }
    }

    fn resolve_thread(&self, id: &ThreadId) -> Vec<Action> {
        let Some(book) = self.comments.book() else {
            return Vec::new();
        };
        let Some(thread) = book.thread(id) else {
            return Vec::new();
        };
        let resolution = match thread.resolution {
            review_threads::Resolution::Open => review_threads::Resolution::Resolved,
            review_threads::Resolution::Resolved => review_threads::Resolution::Open,
        };
        vec![Action::Thread(ThreadCommand::SetResolution {
            review_unit: book.review_unit.clone(),
            thread_id: thread.id.clone(),
            resolution,
        })]
    }

    fn retry_thread(&self, thread_id: ThreadId) -> Vec<Action> {
        self.comments.book().map_or_else(Vec::new, |book| {
            vec![Action::Thread(ThreadCommand::Retry {
                review_unit: book.review_unit.clone(),
                thread_id,
            })]
        })
    }
}
