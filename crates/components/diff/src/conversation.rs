//! A conversation owns its viewport while the file document stays untouched.

use std::cell::RefCell;

use review_threads::{Resolution, ReviewThread, ThreadCommand, ThreadId};
use ui_actions::Action;
use ui_events::{
    PointerInput, PointerInputKind, ReviewNavigation, ReviewNavigationChanged, ReviewPane,
    ReviewPaneFocusRequested, ThreadSelectionChanged,
};
use ui_shortcuts::Key;

use crate::DiffComponent;
use crate::comments::CommentTarget;

mod context;
mod peek;
mod render;
use peek::SourcePeek;

#[derive(Default)]
pub(super) struct ConversationView {
    pub(super) active: bool,
    selected: Option<ThreadId>,
    scroll: usize,
    pub(super) peek: Option<SourcePeek>,
    next_peek: u64,
    targets: RefCell<Vec<Option<CommentTarget>>>,
    unavailable: std::collections::HashSet<ThreadId>,
    original: Option<context::OriginalCode>,
}

#[derive(Clone)]
pub(super) enum ConversationAction {
    Back,
    Reply,
    Editor(crate::comments::EditorAction),
    Resolve(ThreadId),
    Retry(ThreadId),
    Peek,
    OpenFile,
    ClosePeek,
    Read,
}

impl ConversationView {
    pub(super) fn is_peeking(&self) -> bool {
        self.peek.is_some()
    }

    pub(super) fn refresh_files(&mut self) {
        self.unavailable.clear();
    }

    pub(super) fn reset_review(&mut self) {
        self.selected = None;
        self.scroll = 0;
        self.peek = None;
        self.unavailable.clear();
        self.original = None;
    }
    pub(super) fn handles_key(key: Key) -> bool {
        matches!(
            key,
            Key::Down
                | Key::Up
                | Key::PageDown
                | Key::PageUp
                | Key::HalfPageDown
                | Key::HalfPageUp
                | Key::First
                | Key::Last
                | Key::Enter
                | Key::Escape
                | Key::Left
                | Key::Right
                | Key::Space
                | Key::Char('j' | 'k' | 'h' | 'l' | 'a' | 'A' | 'r' | 'p' | 'u')
        )
    }
}

impl DiffComponent {
    pub(super) fn refresh_conversation_context(&mut self) {
        if let Some(thread) = self.conversation_thread()
            && self
                .conversation
                .original
                .as_ref()
                .is_none_or(|code| code.thread != thread.id)
        {
            self.conversation.original =
                Some(context::OriginalCode::new(thread, &self.highlighter));
        }
    }
    fn thread_context(&self, thread: &ReviewThread) -> ui_events::ThreadContext {
        use ui_events::ThreadContext;
        if self.conversation.unavailable.contains(&thread.id) {
            return ThreadContext::Unavailable;
        }
        let Some(file) = self
            .documents
            .iter()
            .find(|file| !file.comments_only && self.comments.matches_path(file, thread.path()))
        else {
            return ThreadContext::OutsideDiff;
        };
        if file.content.is_none() {
            return ThreadContext::Original;
        }
        if self.comments.mapped(&thread.id).is_none() {
            return ThreadContext::Earlier;
        }
        if file.document.diff.is_empty() || self.comments.thread_row(thread, file).1 {
            return ThreadContext::Hidden;
        }
        ThreadContext::Current
    }

    pub(super) fn publish_thread_contexts(&self) {
        if let Some(book) = &self.comments.book {
            self.events.publish(ui_events::ThreadContextsChanged {
                review_unit: book.review_unit.clone(),
                contexts: book
                    .threads()
                    .iter()
                    .map(|thread| (thread.id.clone(), self.thread_context(thread)))
                    .collect(),
            });
        }
    }
    pub(super) fn conversation_thread(&self) -> Option<&ReviewThread> {
        self.comments
            .book
            .as_ref()?
            .thread(self.conversation.selected.as_ref()?)
    }

    pub(super) fn editor_is_visible(&self) -> bool {
        if self.conversation.is_peeking() {
            return false;
        }
        let Some(editing) = &self.comments.editing else {
            return false;
        };
        if self.conversation.active {
            return self.conversation_thread().is_some_and(|thread| {
                thread
                    .messages
                    .iter()
                    .any(|message| editing.draft.reply_to.as_ref() == Some(&message.id))
            });
        }
        self.selected_document()
            .is_some_and(|file| self.comments.inline_editor_visible_in(file))
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(super) fn conversation_navigation(&mut self, event: &ReviewNavigationChanged) {
        let active = event.0 == ReviewNavigation::Threads;
        if active != self.conversation.active {
            if active {
                self.comments.enter_conversations();
                if let Some(id) = &self.conversation.selected {
                    self.comments.restore_thread_editor(id.clone());
                }
            } else {
                self.comments.leave_conversations();
                self.close_peek();
            }
        }
        self.conversation.active = active;
    }

    pub(super) fn conversation_selected(&mut self, event: &ThreadSelectionChanged) -> Vec<Action> {
        if self.conversation.selected != event.thread_id {
            self.conversation.selected.clone_from(&event.thread_id);
            self.conversation.scroll = 0;
            self.close_peek();
            if self.conversation.active {
                if let Some(id) = &event.thread_id {
                    self.comments.restore_thread_editor(id.clone());
                } else {
                    self.comments.park_editor();
                    self.conversation.original = None;
                }
            }
        }
        self.refresh_conversation_context();
        Vec::new()
    }

    fn read_conversation(&self) -> Vec<Action> {
        let Some(book) = &self.comments.book else {
            return Vec::new();
        };
        let Some(thread) = self
            .conversation_thread()
            .filter(|thread| thread.has_unread_replies())
        else {
            return Vec::new();
        };
        vec![Action::Thread(ThreadCommand::MarkRead {
            review_unit: book.review_unit.clone(),
            thread_id: thread.id.clone(),
            through: book.sequence(),
        })]
    }

    pub(super) fn conversation_action(&mut self, action: ConversationAction) -> Vec<Action> {
        match action {
            ConversationAction::Back => self
                .events
                .publish(ReviewNavigationChanged(ReviewNavigation::Files)),
            ConversationAction::ClosePeek => self.close_peek(),
            ConversationAction::Peek => return self.peek_conversation(),
            ConversationAction::OpenFile => self.open_conversation_file(),
            ConversationAction::Read => return self.read_conversation(),
            ConversationAction::Resolve(id) => return self.resolve_thread(&id),
            ConversationAction::Retry(id) => return self.retry_thread(id),
            ConversationAction::Reply => self.reply_to_conversation(),
            ConversationAction::Editor(action) => return self.finish_comment(action),
        }
        Vec::new()
    }

    fn open_conversation_file(&self) {
        let Some(thread) = self.conversation_thread() else {
            return;
        };
        let path = self
            .documents
            .iter()
            .find(|file| self.comments.matches_path(file, thread.path()))
            .map_or_else(|| thread.path().to_owned(), |file| file.path.clone());
        self.events
            .publish(ReviewNavigationChanged(ReviewNavigation::Files));
        self.events
            .publish(ui_events::FileSelectionRequested { path });
        self.events
            .publish(ReviewPaneFocusRequested(ReviewPane::Detail));
    }

    fn reply_to_conversation(&mut self) {
        if let Some(id) = self
            .conversation_thread()
            .and_then(|thread| thread.messages.last())
            .map(|message| message.id.clone())
        {
            self.comments.start_reply(id);
            self.keep_conversation_composer_visible();
        }
    }

    fn resolve_thread(&self, id: &ThreadId) -> Vec<Action> {
        let Some(book) = &self.comments.book else {
            return Vec::new();
        };
        let Some(thread) = book.thread(id) else {
            return Vec::new();
        };
        let resolution = match thread.resolution {
            Resolution::Open => Resolution::Resolved,
            Resolution::Resolved => Resolution::Open,
        };
        vec![Action::Thread(ThreadCommand::SetResolution {
            review_unit: book.review_unit.clone(),
            thread_id: thread.id.clone(),
            resolution,
        })]
    }

    fn retry_thread(&self, thread_id: ThreadId) -> Vec<Action> {
        self.comments.book.as_ref().map_or_else(Vec::new, |book| {
            vec![Action::Thread(ThreadCommand::Retry {
                review_unit: book.review_unit.clone(),
                thread_id,
            })]
        })
    }

    pub(super) fn conversation_key(&mut self, key: Key) -> Vec<Action> {
        let page = isize::try_from(self.viewport_height)
            .unwrap_or(isize::MAX)
            .saturating_sub(2)
            .max(1);
        match key {
            Key::Down | Key::Char('j') => self.scroll_conversation(1),
            Key::Up | Key::Char('k') => self.scroll_conversation(-1),
            Key::PageDown | Key::HalfPageDown | Key::Space => self.scroll_conversation(page),
            Key::PageUp | Key::HalfPageUp => self.scroll_conversation(-page),
            Key::First => self.scroll_conversation(isize::MIN),
            Key::Last => self.scroll_conversation(isize::MAX),
            _ => return self.conversation_action_key(key),
        }
        Vec::new()
    }

    fn conversation_action_key(&mut self, key: Key) -> Vec<Action> {
        let action = match key {
            Key::Escape if self.conversation.peek.is_some() => ConversationAction::ClosePeek,
            Key::Escape => ConversationAction::Back,
            Key::Char('a' | 'A') if self.conversation.peek.is_none() => ConversationAction::Reply,
            Key::Char('r') if self.conversation.peek.is_none() => {
                return self
                    .conversation
                    .selected
                    .as_ref()
                    .map_or_else(Vec::new, |id| self.resolve_thread(id));
            }
            Key::Char('p') => ConversationAction::Peek,
            Key::Char('u') | Key::Enter => ConversationAction::Read,
            Key::Left | Key::Char('h') => {
                self.events
                    .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
                return Vec::new();
            }
            _ => return Vec::new(),
        };
        self.conversation_action(action)
    }

    fn scroll_conversation(&mut self, delta: isize) {
        let limit = self.conversation_scroll_limit();
        let scroll = &mut self.conversation.scroll;
        *scroll = scroll.saturating_add_signed(delta).min(limit);
    }

    pub(super) fn keep_conversation_composer_visible(&mut self) {
        if !self.editor_is_visible() || self.conversation.peek.is_some() {
            return;
        }
        self.conversation.scroll = self
            .conversation
            .scroll
            .min(self.conversation_scroll_limit());
    }

    pub(super) fn conversation_pointer(&mut self, input: PointerInput) -> Vec<Action> {
        if let PointerInputKind::Scroll(delta) = input.kind {
            self.scroll_conversation(delta);
            return Vec::new();
        }
        if !matches!(input.kind, PointerInputKind::Click) {
            return Vec::new();
        }
        let Some(position) = input.position else {
            return Vec::new();
        };
        let row = usize::from(position.component_row.saturating_sub(1));
        let target = self
            .conversation
            .targets
            .borrow()
            .get(row)
            .cloned()
            .flatten();
        target.map_or_else(Vec::new, |target| {
            self.activate_comment_target(
                target,
                usize::from(position.component_column.saturating_sub(1)),
            )
        })
    }
}
