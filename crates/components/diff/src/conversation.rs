//! The pane shows one review thread as a conversation while the Files viewer
//! keeps its document untouched.

use std::cell::RefCell;

use component_core::InputResolution;
use review_threads::{MessageId, ReviewThread, ThreadCommand, ThreadId};
use ui_actions::Action;
use ui_events::{
    PointerInput, PointerInputKind, ReviewNavigation, ReviewNavigationChanged, ReviewPane,
    ReviewPaneFocusRequested, TextPasted, ThreadSelectionChanged,
};
use ui_shortcuts::{ConversationCommand, ConversationShortcut, Key, ShortcutMatcher};

use crate::DiffComponent;
use crate::comments::{CommentTarget, ConversationButton};

mod context;
mod peek;
mod render;
pub(super) use peek::SourcePeek;

#[derive(Default)]
pub(super) struct ConversationView {
    /// The pane shows the selected thread instead of the Files diff.
    pub(super) active: bool,
    selected: Option<ThreadId>,
    scroll: usize,
    /// What the view keeps in sight as the thread opens, until the reviewer scrolls: the rows
    /// above can still grow, as the thread's code loads.
    anchor: Option<ConversationAnchor>,
    /// Whether the reviewer scrolled the thread since it was selected: coming back to the
    /// Threads tab then keeps where they were.
    scrolled: bool,
    /// The shown thread laid out for the pane's width: a frame and a scroll step reuse it.
    laid_out: RefCell<Option<render::LaidOutThread>>,
    pub(super) peek: Option<SourcePeek>,
    next_peek: u64,
    targets: RefCell<Vec<Option<CommentTarget>>>,
    original: Option<context::OriginalCode>,
}

/// Where the view of a thread that just opened stays.
enum ConversationAnchor {
    /// The top of the thread's first unread reply, at the top of the view.
    Reply(MessageId),
    /// The end of a thread without unread replies, at the bottom of the view.
    End,
}

#[derive(Clone)]
pub(super) enum ConversationAction {
    Back,
    Reply,
    Editor(crate::comments::EditorAction, review_drafts::DraftId),
    Resolve(ThreadId),
    Retry(ThreadId),
    Peek,
    OpenFile,
    ClosePeek,
    Read,
}

impl ConversationView {
    /// Forget the thread shown for a review that is no longer shown.
    pub(super) fn reset_review(&mut self) {
        self.selected = None;
        self.scroll = 0;
        self.anchor = None;
        self.scrolled = false;
        self.peek = None;
        self.original = None;
    }

    /// Whether `key` is bound to leaving the conversation view, which closes
    /// an open source peek before the peek's own keys apply.
    pub(super) fn goes_back(key: Key) -> bool {
        ShortcutMatcher::<ConversationShortcut>::new().resolve_key(key)
            == InputResolution::Matched(ConversationShortcut::Back)
    }
}

impl DiffComponent {
    pub(super) fn refresh_conversation_context(&mut self) {
        let Some(thread) = self.conversation_thread() else {
            return;
        };
        let Some((path, code)) = thread.path().zip(thread.code()) else {
            // A round conversation discusses no code.
            self.conversation.original = None;
            return;
        };
        if self
            .conversation
            .original
            .as_ref()
            .is_none_or(|original| original.thread != thread.id)
        {
            self.conversation.original = Some(context::OriginalCode::new(
                thread.id.clone(),
                path,
                code,
                &self.services.highlighter,
            ));
        }
    }

    pub(super) fn conversation_thread(&self) -> Option<&ReviewThread> {
        self.files
            .comments
            .book()?
            .thread(self.conversation.selected.as_ref()?)
    }

    /// Whether the conversation shows the editor of a reply to its thread.
    pub(super) fn conversation_editor_visible(&self) -> bool {
        self.conversation.peek.is_none()
            && self.files.comments.focused().is_some()
            && self
                .conversation_thread()
                .is_some_and(|thread| self.files.comments.replies_to(thread))
    }

    /// Show the conversation instead of the Files diff, or the reverse.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(super) fn conversation_navigation(&mut self, event: &ReviewNavigationChanged) {
        let active = event.0 == ReviewNavigation::Threads;
        if active != self.conversation.active {
            if active {
                if !self.conversation.scrolled {
                    self.open_shown_thread();
                }
                self.files.comments.enter_conversations();
                if let Some(id) = &self.conversation.selected {
                    self.files.comments.restore_thread_editor(id);
                }
            } else {
                self.files.comments.leave_conversations();
                self.close_peek();
            }
        }
        self.conversation.active = active;
    }

    pub(super) fn conversation_selected(&mut self, event: &ThreadSelectionChanged) -> Vec<Action> {
        // Explore hides the conversation, which keeps the thread it showed.
        if self.explore_shown() {
            return Vec::new();
        }
        if self.conversation.selected != event.thread_id {
            self.conversation.selected.clone_from(&event.thread_id);
            self.conversation.scroll = 0;
            self.conversation.anchor = None;
            self.conversation.scrolled = false;
            self.close_peek();
            if self.conversation.active {
                self.open_shown_thread();
                if let Some(id) = &event.thread_id {
                    self.files.comments.restore_thread_editor(id);
                } else {
                    self.files.comments.park_editor();
                    self.conversation.original = None;
                }
            }
        }
        self.refresh_conversation_context();
        Vec::new()
    }

    /// Keeps the top of the shown thread's first unread reply at the top of the view, as the
    /// thread opens. A thread without one keeps where it opened, though its replies were read
    /// since, or else its end in view.
    fn open_shown_thread(&mut self) {
        let Some(thread) = self.conversation_thread() else {
            return;
        };
        match thread.first_unread_reply() {
            Some(reply) => {
                self.conversation.anchor = Some(ConversationAnchor::Reply(reply.id.clone()));
            }
            None => {
                self.conversation
                    .anchor
                    .get_or_insert(ConversationAnchor::End);
            }
        }
    }

    /// Opens the shown thread once it loads, and shows a reply that arrives while the view keeps
    /// the thread's end from its top. A reply arriving as the reviewer writes leaves the view.
    pub(super) fn follow_arriving_replies(&mut self) {
        if self.conversation.active
            && !self.conversation.scrolled
            && !self.conversation_editor_visible()
            && !matches!(self.conversation.anchor, Some(ConversationAnchor::Reply(_)))
        {
            self.open_shown_thread();
        }
    }

    /// Reopen the reply saved for the shown thread once its drafts loaded.
    pub(super) fn restore_conversation_editor(&mut self) {
        if self.files.comments.focused_id().is_none()
            && self.conversation.active
            && let Some(thread) = self.conversation_thread().map(|thread| thread.id.clone())
        {
            self.files.comments.restore_thread_editor(&thread);
        }
    }

    fn read_conversation(&self) -> Vec<Action> {
        let Some(book) = self.files.comments.book() else {
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

    fn conversation_action(&mut self, action: ConversationAction) -> Vec<Action> {
        match action {
            ConversationAction::Back => self
                .services
                .events
                .publish(ReviewNavigationChanged(ReviewNavigation::Files)),
            ConversationAction::ClosePeek => self.close_peek(),
            ConversationAction::Peek => return self.peek_conversation(),
            ConversationAction::OpenFile => self.open_conversation_file(),
            ConversationAction::Read => return self.read_conversation(),
            ConversationAction::Reply => self.reply_to_conversation(),
            ConversationAction::Resolve(_)
            | ConversationAction::Retry(_)
            | ConversationAction::Editor(..) => return self.files.comment_action(action),
        }
        Vec::new()
    }

    /// The shown thread and the path its code was saved on; a round conversation has none.
    pub(super) fn conversation_code_thread(&self) -> Option<(&ReviewThread, &str)> {
        let thread = self.conversation_thread()?;
        Some((thread, thread.path()?))
    }

    fn open_conversation_file(&self) {
        let Some((thread, saved_path)) = self.conversation_code_thread() else {
            return;
        };
        let path = self
            .files
            .documents
            .iter()
            .find(|file| self.files.comments.shows_thread(file, thread))
            .map_or_else(|| saved_path.to_owned(), |file| file.path.clone());
        let events = &self.services.events;
        events.publish(ReviewNavigationChanged(ReviewNavigation::Files));
        events.publish(ui_events::FileSelectionRequested { path });
        events.publish(ReviewPaneFocusRequested(ReviewPane::Detail));
    }

    fn reply_to_conversation(&mut self) {
        if let Some(id) = self
            .conversation_thread()
            .and_then(|thread| thread.messages.last())
            .map(|message| message.id.clone())
        {
            self.files.comments.start_reply(id);
            self.keep_conversation_composer_visible();
        }
    }

    pub(super) fn conversation_command(&mut self, command: ConversationCommand) -> Vec<Action> {
        match command {
            ConversationCommand::Movement(command) => {
                self.scroll_conversation(command.row_delta(self.conversation_page()));
                Vec::new()
            }
            ConversationCommand::Conversation(command) => self.run_conversation_shortcut(command),
        }
    }

    fn conversation_page(&self) -> isize {
        isize::try_from(self.files.viewport_height)
            .unwrap_or(isize::MAX)
            .saturating_sub(2)
            .max(1)
    }

    fn run_conversation_shortcut(&mut self, command: ConversationShortcut) -> Vec<Action> {
        let peeking = self.conversation.peek.is_some();
        let action = match command {
            ConversationShortcut::Back if peeking => ConversationAction::ClosePeek,
            ConversationShortcut::Back => ConversationAction::Back,
            ConversationShortcut::Reply if !peeking => ConversationAction::Reply,
            ConversationShortcut::ToggleResolution if !peeking => {
                let Some(id) = self.conversation.selected.clone() else {
                    return Vec::new();
                };
                ConversationAction::Resolve(id)
            }
            ConversationShortcut::Peek => ConversationAction::Peek,
            ConversationShortcut::MarkRead => ConversationAction::Read,
            ConversationShortcut::FocusThreads => {
                self.services
                    .events
                    .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
                return Vec::new();
            }
            ConversationShortcut::Reply | ConversationShortcut::ToggleResolution => {
                return Vec::new();
            }
        };
        self.conversation_action(action)
    }

    fn scroll_conversation(&mut self, delta: isize) {
        self.settle_conversation_scroll();
        self.conversation.scrolled = true;
        let limit = self.conversation_scroll_limit();
        let scroll = &mut self.conversation.scroll;
        *scroll = scroll.saturating_add_signed(delta).min(limit);
    }

    pub(super) fn keep_conversation_composer_visible(&mut self) {
        if !self.conversation_editor_visible() {
            return;
        }
        self.settle_conversation_scroll();
        self.conversation.scroll = self
            .conversation
            .scroll
            .min(self.conversation_scroll_limit());
    }

    /// Paste into the reply the conversation edits.
    pub(super) fn conversation_paste(&mut self, input: &TextPasted) -> Vec<Action> {
        if !self.conversation_editor_visible() {
            return Vec::new();
        }
        self.files.paste_into_draft(input)
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
            self.activate_conversation_target(
                target,
                usize::from(position.component_column.saturating_sub(1)),
            )
        })
    }

    fn activate_conversation_target(
        &mut self,
        target: CommentTarget,
        column: usize,
    ) -> Vec<Action> {
        match target {
            CommentTarget::Conversation(action) => return self.conversation_action(action),
            CommentTarget::ConversationButtons(buttons) => {
                if let Some(action) = ConversationButton::at(buttons, column) {
                    return self.conversation_action(action);
                }
            }
            CommentTarget::Message(id) => self.files.open_comment(id),
            CommentTarget::Reply(id) => {
                self.files.start_comment(id);
                self.keep_conversation_composer_visible();
            }
            CommentTarget::Draft(draft) => {
                self.files.open_draft(draft);
                self.keep_conversation_composer_visible();
            }
        }
        Vec::new()
    }
}
