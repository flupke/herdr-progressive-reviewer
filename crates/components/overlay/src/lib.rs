//! Modal overlays and application notifications.

mod commit_message;
mod context_menu;
mod hover;
mod popup;
mod shortcut_help;

use component_core::{AnyInput, Component, ComponentSubscriptions, InputScope};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use review_lsp::{Event as LspEvent, Operation, Query};
use syntax_highlighting::SyntaxHighlighter;
use toasts::{ToastId, ToastKind, ToastState};
use ui_actions::Action;
use ui_events::{
    CommitMessageToggleRequested, ContextMenuRequested, LspQueryContext, LspQueryRequested,
    PointerInput, PointerInputKind, RepositoryMetadataChanged, ReviewGuideStatusChanged,
    ToastExpirationTick, ToastRequested, ViewportChanged,
};
use ui_shortcuts::{ApplicationShortcut, Key, ShortcutCommand, ShortcutMatcher, ShortcutSet};
use ui_theme::{Palette, Theme};

use commit_message::CommitMessageOverlay;
use context_menu::SourceContextMenu;
use hover::HoverOverlay;
use shortcut_help::ShortcutHelpOverlay;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModalOverlay {
    CommitMessage,
    ShortcutHelp,
}

/// Application overlays with their input, state, and rendering.
pub struct OverlayComponent {
    snapshot_id: String,
    active_modal: Option<ModalOverlay>,
    commit_message: CommitMessageOverlay,
    shortcut_help: ShortcutHelpOverlay,
    hover: HoverOverlay,
    source_context_menu: Option<SourceContextMenu>,
    toasts: ToastState,
    lsp_initialization_toast: Option<ToastId>,
    viewport: Rect,
    palette: Palette,
}

impl OverlayComponent {
    pub fn new(theme: Theme) -> Self {
        Self {
            snapshot_id: String::new(),
            active_modal: None,
            commit_message: CommitMessageOverlay::default(),
            shortcut_help: ShortcutHelpOverlay::default(),
            hover: HoverOverlay::new(SyntaxHighlighter::new(theme.syntax, theme.palette.text)),
            source_context_menu: None,
            toasts: ToastState::default(),
            lsp_initialization_toast: None,
            viewport: Rect::new(0, 0, 80, 24),
            palette: theme.palette,
        }
    }

    pub fn is_modal(&self) -> bool {
        self.active_modal.is_some() || self.hover.is_open() || self.source_context_menu.is_some()
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        self.hover.render(area, buffer, self.palette);
        self.toasts
            .render(area, buffer, self.palette.focus, self.palette.deletion);
        match self.active_modal {
            Some(ModalOverlay::CommitMessage) => {
                self.commit_message.render(area, buffer, self.palette);
            }
            Some(ModalOverlay::ShortcutHelp) => {
                self.shortcut_help.render(area, buffer, self.palette);
            }
            None => {}
        }
        if let Some(menu) = &self.source_context_menu {
            menu.render(area, buffer, self.palette);
        }
    }

    fn repository_changed(&mut self, event: &RepositoryMetadataChanged) {
        self.commit_message.replace_description(&event.description);
        self.snapshot_id
            .clone_from(&event.review_checkpoint.checkpoint);
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn viewport_changed(&mut self, event: &ViewportChanged) {
        self.viewport = Rect::new(0, 0, event.width, event.height);
        self.shortcut_help.resize(event.height);
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn toggle_commit_message(&mut self, _event: &CommitMessageToggleRequested) {
        self.active_modal = (self.active_modal != Some(ModalOverlay::CommitMessage))
            .then_some(ModalOverlay::CommitMessage);
    }

    fn guide_status_changed(&mut self, event: &ReviewGuideStatusChanged) {
        if let Some(message) = &event.message {
            self.toasts.push(message, ToastKind::Error);
        }
    }

    fn toast_requested(&mut self, event: &ToastRequested) {
        self.toasts.push(&event.text, event.kind);
    }

    fn context_menu_requested(&mut self, event: &ContextMenuRequested) {
        self.source_context_menu = Some(SourceContextMenu {
            column: event.column,
            row: event.row,
            selected: 0,
            query: event.query.clone(),
        });
    }

    fn lsp_query_requested(&mut self, event: &LspQueryRequested) -> Vec<Action> {
        vec![self.start_lsp_query(event.operation, event.query.clone())]
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn expiration_tick(&mut self, event: &ToastExpirationTick) {
        self.toasts.expire(event.now);
    }

    fn lsp_event(&mut self, event: &LspEvent) {
        if !self.lsp_event_matches_snapshot(event) {
            return;
        }
        match event {
            LspEvent::Initializing => {
                self.finish_lsp_initialization();
                self.lsp_initialization_toast =
                    Some(self.toasts.start_long_toast("Starting rust-analyzer..."));
            }
            LspEvent::Ready => {
                self.finish_lsp_initialization();
                self.toasts.push("rust-analyzer ready", ToastKind::Info);
            }
            LspEvent::Failed {
                toast_id, message, ..
            } => {
                self.finish_lsp_initialization();
                if let Some(toast_id) = toast_id {
                    self.toasts.finish_toast(*toast_id);
                }
                self.toasts.push(message, ToastKind::Error);
            }
            LspEvent::Hover {
                toast_id, markdown, ..
            } => {
                self.toasts.finish_toast(*toast_id);
                self.hover.replace_markdown(markdown.as_ref());
                if markdown.is_none() {
                    self.toasts.push("No documentation found", ToastKind::Info);
                }
            }
            LspEvent::Locations { toast_id, .. } => self.toasts.finish_toast(*toast_id),
        }
    }

    fn lsp_event_matches_snapshot(&self, event: &LspEvent) -> bool {
        match event {
            LspEvent::Initializing | LspEvent::Ready => true,
            LspEvent::Hover { snapshot_id, .. } | LspEvent::Locations { snapshot_id, .. } => {
                snapshot_id == &self.snapshot_id
            }
            LspEvent::Failed { snapshot_id, .. } => snapshot_id
                .as_ref()
                .is_none_or(|snapshot_id| snapshot_id == &self.snapshot_id),
        }
    }

    fn keyboard_input(&mut self, key: Key) -> Vec<Action> {
        if self.source_context_menu.is_some() {
            return self.source_context_menu_key(key);
        }
        if self.hover.is_open() {
            return self.hover_key(key);
        }
        if let Some(active_modal) = self.active_modal {
            return self.modal_key(active_modal, key);
        }
        Vec::new()
    }

    fn hover_key(&mut self, key: Key) -> Vec<Action> {
        self.hover.handle_key(key);
        Vec::new()
    }

    fn modal_key(&mut self, active_modal: ModalOverlay, key: Key) -> Vec<Action> {
        match active_modal {
            ModalOverlay::CommitMessage => {
                if matches!(key, Key::CommitMessage | Key::Escape | Key::Char('c')) {
                    self.active_modal = None;
                }
            }
            ModalOverlay::ShortcutHelp => self.shortcut_help_key(key),
        }
        Vec::new()
    }

    fn open_overlay(&mut self, shortcut: ShortcutCommand) {
        match shortcut {
            ShortcutCommand::Application(ApplicationShortcut::ShowCommitMessage) => {
                self.active_modal = Some(ModalOverlay::CommitMessage);
            }
            ShortcutCommand::Application(ApplicationShortcut::OpenHelp) => {
                self.active_modal = Some(ModalOverlay::ShortcutHelp);
                self.shortcut_help.open();
            }
            _ => unreachable!("overlay shortcut set is exact"),
        }
    }

    fn pointer_input(&mut self, input: PointerInput) -> Vec<Action> {
        if !matches!(input.kind, PointerInputKind::Click { .. }) {
            return Vec::new();
        }
        let Some(position) = input.position else {
            return Vec::new();
        };
        let column = position.terminal_column;
        let row = position.terminal_row;
        if let Some(menu) = self.source_context_menu.take() {
            let area = menu.area(self.viewport);
            if area.contains(Position::new(column, row))
                && let Some((operation, query)) = menu.query_at_row(row, area)
            {
                return vec![self.start_lsp_query(operation, query)];
            }
            return Vec::new();
        }
        let popup = match self.active_modal {
            Some(ModalOverlay::CommitMessage) => CommitMessageOverlay::area(self.viewport),
            Some(ModalOverlay::ShortcutHelp) => ShortcutHelpOverlay::area(self.viewport),
            None if self.hover.is_open() => HoverOverlay::area(self.viewport),
            None => return Vec::new(),
        };
        if !popup.contains(Position::new(column, row)) {
            self.active_modal = None;
            self.hover.close();
        }
        Vec::new()
    }

    fn source_context_menu_key(&mut self, key: Key) -> Vec<Action> {
        match key {
            Key::Escape => {
                self.source_context_menu = None;
                Vec::new()
            }
            Key::Down | Key::Char('j') => {
                if let Some(menu) = &mut self.source_context_menu {
                    menu.move_down();
                }
                Vec::new()
            }
            Key::Up | Key::Char('k') => {
                if let Some(menu) = &mut self.source_context_menu {
                    menu.move_up();
                }
                Vec::new()
            }
            Key::Enter => {
                let action = self
                    .source_context_menu
                    .take()
                    .and_then(SourceContextMenu::selected_query)
                    .map(|(operation, query)| self.start_lsp_query(operation, query));
                action.into_iter().collect()
            }
            _ => Vec::new(),
        }
    }

    fn start_lsp_query(&mut self, operation: Operation, query: LspQueryContext) -> Action {
        let toast_id = self.toasts.start_long_toast(operation.progress_text());
        Action::Lsp {
            operation,
            query: Query {
                toast_id,
                path: query.path,
                line: query.line,
                byte_column: query.byte_column,
                expected_line: query.expected_line,
                snapshot_id: query.snapshot_id,
            },
        }
    }

    fn shortcut_help_key(&mut self, key: Key) {
        if self.shortcut_help.handle_key(key, self.viewport.height) {
            self.active_modal = None;
        }
    }

    fn finish_lsp_initialization(&mut self) {
        if let Some(id) = self.lsp_initialization_toast.take() {
            self.toasts.finish_toast(id);
        }
    }
}

impl Component<Action> for OverlayComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::repository_changed);
        subscriptions.subscribe(Self::viewport_changed);
        subscriptions.subscribe(Self::toggle_commit_message);
        subscriptions.subscribe(Self::guide_status_changed);
        subscriptions.subscribe(Self::toast_requested);
        subscriptions.subscribe(Self::context_menu_requested);
        subscriptions.subscribe(Self::lsp_query_requested);
        subscriptions.subscribe(Self::expiration_tick);
        subscriptions.subscribe(Self::lsp_event);
        subscriptions.subscribe_input(InputScope::Focused, AnyInput, Self::keyboard_input);
        subscriptions.subscribe_input(
            InputScope::Global,
            ShortcutMatcher::new(ShortcutSet::Overlay),
            Self::open_overlay,
        );
        subscriptions.subscribe_input(InputScope::Hovered, AnyInput, Self::pointer_input);
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
