//! The diff pane: every source viewer, and the one policy that decides which
//! of them receives an event.
//!
//! Input goes to the viewer the reviewer looks at. Document events go to the
//! viewer the pane shows as its document. Asynchronous results go to every
//! viewer, each of which ignores what is not its own.

use std::path::{Path, PathBuf};

use comment_editor::KeymapSetting;
use component_core::{
    Component, ComponentSubscriptions, EventPublisher, InputMatcher, InputResolution, InputScope,
    IntoDispatchResult,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ui_actions::Action;
use ui_events::{
    DiffViewportChanged, EvidenceView, ExploreComparisonAccepted, ExploreEvidence,
    ExplorePositionsRestored, PointerInput, PointerInputKind, RepositoryFilesChanged,
    ReviewNavigation, ReviewNavigationChanged, ReviewThreadsLoaded, ReviewableFiles,
    SourceContentLoadFailed, SourceContentLoaded, TextPasted, ThreadPostFinished, ToastRequested,
};
use ui_shortcuts::{
    ApplicationShortcut, ConversationCommand, ConversationShortcut, DiffGlobalShortcut,
    DiffPaneCommand, Key, ShortcutMatcher, ShortcutSubscription,
};
use ui_theme::Palette;

use crate::conversation::{ConversationView, SourcePeek};
use crate::embedded::ExploreViewers;
use crate::{Role, Services, SourceViewer, SyntaxHighlighter, ViewerInput};

/// The diff pane of the review: the Files viewer, the conversation shown in
/// its place with an optional source peek, and Explore's evidence viewers.
pub struct DiffComponent {
    pub(super) services: Services,
    pub(super) files: SourceViewer,
    pub(super) conversation: ConversationView,
    explore: ExploreViewers,
    /// The live source the peek last asked the application to watch.
    watched_source: Option<PathBuf>,
}

impl DiffComponent {
    /// Create an empty diff pane.
    pub fn new(
        events: EventPublisher,
        reviewable_files: ReviewableFiles,
        highlighter: SyntaxHighlighter,
        repository_root: PathBuf,
        palette: Palette,
    ) -> Self {
        let services = Services {
            events,
            highlighter,
            repository_root,
            palette,
            drafts: std::rc::Rc::default(),
        };
        let files = SourceViewer::new(&services, Role::Files, reviewable_files);
        Self {
            services,
            files,
            conversation: ConversationView::default(),
            explore: ExploreViewers::default(),
            watched_source: None,
        }
    }

    /// Share the application-wide editor keymap with every comment editor.
    #[must_use]
    pub fn with_editor_keymap(self, keymap: KeymapSetting) -> Self {
        self.services.drafts.borrow_mut().use_keymap(keymap);
        self
    }

    /// File selection defers loading to the next application tick.
    pub fn has_pending_load(&self) -> bool {
        self.document_viewer().has_pending_load()
    }

    /// Draw what the pane shows, and note which reply rows it drew.
    pub fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette, focused: bool) {
        let viewer = if let Some(viewer) = self.shown_explore_viewer() {
            viewer.render(area, buffer, palette, focused).render(buffer);
            viewer
        } else if let Some(peek) = &self.conversation.peek {
            peek.render(area, buffer, palette, focused);
            peek.viewer()
        } else if self.conversation.active {
            self.render_conversation(area, buffer, palette, focused);
            &self.files
        } else {
            self.files
                .render(area, buffer, palette, focused)
                .render(buffer);
            &self.files
        };
        viewer.capture_replies(buffer);
    }

    /// Borrow the viewer retained for an opened evidence block.
    pub fn evidence_view(&self, id: EvidenceView) -> Option<&SourceViewer> {
        self.explore
            .is_shown()
            .then(|| self.explore.view(id))
            .flatten()
    }

    /// The position of every question's evidence viewer, for Explore to save.
    pub fn explore_positions(&self) -> Vec<review_explore::EvidencePosition> {
        self.explore.positions()
    }

    /// Start observations for a fresh frame, including frames with no detail pane.
    pub fn begin_reply_frame(&self) {
        for viewer in self.viewers() {
            viewer.begin_replies();
        }
    }

    /// Exclude rows covered by a popup, menu, notification, or other overlay.
    pub fn finish_reply_frame(&self, buffer: &Buffer) {
        for viewer in self.viewers() {
            viewer.finish_replies(buffer);
        }
    }
}

/// Routing: which viewer receives an event.
impl DiffComponent {
    fn shown_explore_viewer(&self) -> Option<&SourceViewer> {
        self.explore
            .is_shown()
            .then(|| self.explore.current())
            .flatten()
    }

    /// The peek, unless Explore hides the conversation it belongs to.
    fn visible_peek(&self) -> Option<&SourcePeek> {
        if self.explore.is_shown() {
            return None;
        }
        self.conversation.peek.as_ref()
    }

    /// The peek, unless Explore hides the conversation it belongs to.
    pub(super) fn visible_peek_mut(&mut self) -> Option<&mut SourcePeek> {
        if self.explore.is_shown() {
            return None;
        }
        self.conversation.peek.as_mut()
    }

    /// The viewer the pane shows as its document: the Explore viewer in
    /// front while Explore is shown, the Files viewer otherwise.
    fn document_viewer(&self) -> &SourceViewer {
        self.shown_explore_viewer().unwrap_or(&self.files)
    }

    fn document_viewer_mut(&mut self) -> &mut SourceViewer {
        if self.explore.is_shown()
            && let Some(viewer) = self.explore.current_mut()
        {
            return viewer;
        }
        &mut self.files
    }

    /// The viewer the reviewer looks at: the peek over the conversation, or
    /// the document viewer.
    fn input_viewer(&self) -> &SourceViewer {
        self.visible_peek()
            .map_or_else(|| self.document_viewer(), SourcePeek::viewer)
    }

    fn input_viewer_mut(&mut self) -> &mut SourceViewer {
        if self.visible_peek().is_none() {
            return self.document_viewer_mut();
        }
        self.visible_peek_mut()
            .expect("the peek is shown")
            .viewer_mut()
    }

    pub(super) fn explore_shown(&self) -> bool {
        self.explore.is_shown()
    }

    /// Whether the pane shows the conversation itself, not a viewer over it.
    fn conversation_shown(&self) -> bool {
        !self.explore.is_shown() && self.conversation.active && self.conversation.peek.is_none()
    }

    fn viewers(&self) -> impl Iterator<Item = &SourceViewer> {
        std::iter::once(&self.files)
            .chain(self.conversation.peek.iter().map(SourcePeek::viewer))
            .chain(self.explore.viewers())
    }

    /// Every viewer, each with whether the pane shows it: the document
    /// viewer and the peek over it. Hidden viewers come first.
    fn viewers_mut(&mut self) -> Vec<(bool, &mut SourceViewer)> {
        let explore_shown = self.explore.is_shown();
        let mut viewers: Vec<(bool, &mut SourceViewer)> = self
            .explore
            .viewers_mut()
            .map(|(current, viewer)| (explore_shown && current, viewer))
            .collect();
        if let Some(peek) = &mut self.conversation.peek {
            viewers.push((!explore_shown, peek.viewer_mut()));
        }
        viewers.push((!explore_shown, &mut self.files));
        viewers.sort_by_key(|(shown, _)| *shown);
        viewers
    }

    /// Deliver the reviewer's input to the viewer they look at.
    fn route<R>(&mut self, deliver: impl FnOnce(&mut SourceViewer) -> R) -> R {
        deliver(self.input_viewer_mut())
    }

    /// Deliver an event about the shown documents to the document viewer.
    /// A peek keeps its own search on the status line meanwhile.
    fn for_document<R>(&mut self, deliver: impl FnOnce(&mut SourceViewer) -> R) -> R {
        let result = deliver(self.document_viewer_mut());
        if let Some(peek) = self.visible_peek_mut() {
            peek.viewer().publish_search_status();
        }
        result
    }

    /// Deliver an asynchronous result to every viewer.
    fn broadcast(
        &mut self,
        mut deliver: impl FnMut(&mut SourceViewer) -> Vec<Action>,
    ) -> Vec<Action> {
        self.viewers_mut()
            .into_iter()
            .flat_map(|(_, viewer)| deliver(viewer))
            .collect()
    }

    /// Run one handler, then ask the application to watch the peek's live
    /// source whenever it changed.
    fn handle<E, R: IntoDispatchResult<Action>>(
        &mut self,
        event: E,
        handler: fn(&mut Self, E) -> R,
    ) -> Vec<Action> {
        let mut actions = handler(self, event).into_dispatch_result().into_actions();
        let source = self.live_source_path().map(Path::to_owned);
        if source != self.watched_source {
            self.watched_source.clone_from(&source);
            actions.push(Action::WatchSource(source));
        }
        actions
    }

    /// The live source whose disk changes must refresh the current peek.
    fn live_source_path(&self) -> Option<&Path> {
        self.visible_peek().map(SourcePeek::source_path)
    }
}

/// Handlers the pane runs itself, around its viewers.
impl DiffComponent {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn navigation_changed(&mut self, event: &ReviewNavigationChanged) -> Vec<Action> {
        let explore = event.0 == ReviewNavigation::Explore;
        let switched = self.explore.is_shown() != explore;
        let mut actions = Vec::new();
        if switched {
            if let Some(latest) = self.explore.show(explore, &self.services) {
                actions.extend(self.repository_changed(&latest));
            }
            self.announce_document_session();
        }
        if !explore {
            self.conversation_navigation(event);
        }
        if !switched {
            return actions;
        }
        if let Some(checkpoint) = &self.document_viewer().review_checkpoint {
            actions.push(Action::Thread(review_threads::ThreadCommand::Load(
                checkpoint.review_unit.clone(),
            )));
        }
        if explore {
            actions.extend(self.document_viewer_mut().resume_evidence_search());
        }
        actions
    }

    /// Tell the application which snapshot the shown document's sources use.
    fn announce_document_session(&self) {
        self.services
            .events
            .publish(ui_events::SourceSessionChanged {
                snapshot_id: self.document_viewer().source_session.clone(),
            });
    }

    fn repository_changed(&mut self, event: &RepositoryFilesChanged) -> Vec<Action> {
        if self.explore.is_shown() {
            self.explore.defer(event);
            return Vec::new();
        }
        if !self.files.shows_review_of(event) {
            self.close_peek();
            self.conversation.reset_review();
        }
        let actions = self.for_document(|viewer| viewer.repository_changed(event));
        self.refresh_peek_checkpoint();
        actions
    }

    fn threads_loaded(&mut self, event: &ReviewThreadsLoaded) -> Vec<Action> {
        let actions = self.broadcast(|viewer| viewer.threads_loaded(event));
        if self.files.shows_review_unit(&event.review_unit) {
            if event.result.is_ok() {
                self.restore_conversation_editor();
            }
            self.refresh_conversation_context();
        }
        if let Err(message) = &event.result {
            self.services.events.publish(ToastRequested {
                text: format!("Could not load comments: {message}"),
                kind: toasts::ToastKind::Error,
            });
        }
        actions
    }

    fn post_finished(&mut self, event: &ThreadPostFinished) {
        let mut released = false;
        for (_, viewer) in self.viewers_mut() {
            released |= viewer.post_finished(event) && viewer.comments.in_conversation();
        }
        if released {
            self.keep_conversation_composer_visible();
        }
        if let Err(error) = &event.result {
            self.services.events.publish(ToastRequested {
                text: format!("Could not post comment: {error}"),
                kind: toasts::ToastKind::Error,
            });
        }
    }

    /// Shown viewers move to the results of their search; hidden ones keep
    /// theirs for later.
    fn search_completed(&mut self, results: &text_search::Results) -> Vec<Action> {
        let mut actions = Vec::new();
        for (shown, viewer) in self.viewers_mut() {
            if shown {
                actions.extend(viewer.complete_search(results));
            } else {
                viewer.retain_search_results(results);
            }
        }
        // The Files viewer completes last; the peek over it keeps the status line.
        if let Some(peek) = self.visible_peek() {
            peek.viewer().publish_search_status();
        }
        actions
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn highlighting_finished(&mut self, event: &ui_events::HighlightingFinished) {
        self.broadcast(|viewer| {
            viewer.highlighting_finished(event);
            Vec::new()
        });
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn replies_displayed(&mut self, _: &ui_events::FrameRendered) -> Vec<Action> {
        self.broadcast(SourceViewer::displayed_reply_actions)
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn viewport_changed(&mut self, event: &DiffViewportChanged) {
        if self.explore.is_shown() {
            self.document_viewer_mut().viewport_changed(event);
            return;
        }
        if let Some(peek) = &mut self.conversation.peek {
            peek.viewer_mut().viewport_changed(&DiffViewportChanged {
                width: event.width,
                height: event.height.saturating_sub(1),
            });
        }
        if self.files.viewport_changed(event) && self.conversation.active {
            self.keep_conversation_composer_visible();
        }
    }

    fn source_content_loaded(&mut self, event: &SourceContentLoaded) -> Vec<Action> {
        if event.mode == ui_events::SourceLoadMode::ThreadPeek {
            return self.peek_loaded(event);
        }
        if let Some(peek) = self.visible_peek_mut()
            && peek.viewer().source_snapshot() == Some(event.snapshot_id.as_str())
        {
            return peek.viewer_mut().source_content_loaded(event);
        }
        self.for_document(|viewer| viewer.source_content_loaded(event))
    }

    fn source_content_failed(&mut self, event: &SourceContentLoadFailed) {
        if !self.peek_failed(event) {
            self.route(|viewer| viewer.source_content_failed(event));
        }
    }

    fn comment_paste(&mut self, input: &TextPasted) -> Vec<Action> {
        if self.conversation_shown() {
            return self.conversation_paste(input);
        }
        if self.visible_peek().is_some() {
            return Vec::new();
        }
        self.document_viewer_mut().comment_paste(input)
    }

    fn explore_comparison_accepted(&mut self, event: &ExploreComparisonAccepted) -> Vec<Action> {
        let comparison = &event.0;
        let accepted = self.explore.accept(comparison.clone());
        if !self.explore.is_shown() || !accepted {
            return Vec::new();
        }
        self.announce_document_session();
        vec![Action::Thread(review_threads::ThreadCommand::Load(
            comparison.checkpoint.review_unit.clone(),
        ))]
    }

    fn explore_evidence(&mut self, event: &ExploreEvidence) -> Vec<Action> {
        if !self.explore.is_shown() {
            return Vec::new();
        }
        match event.view {
            EvidenceView::Coverage => {
                let Some(index) = crate::explore::coverage_file(event) else {
                    return Vec::new();
                };
                let shown = self.explore.activate(&self.services, event);
                self.document_viewer_mut()
                    .show_coverage_evidence(event, index, shown)
            }
            EvidenceView::Question { reference, .. } => {
                let shown = self.explore.activate(&self.services, event);
                self.document_viewer_mut()
                    .show_question_evidence(event, reference, shown)
            }
        }
    }

    fn explore_evidence_input(&mut self, event: &ui_events::ExploreEvidenceInput) -> Vec<Action> {
        if !self.explore.is_shown() {
            return Vec::new();
        }
        self.explore.pointer(event)
    }

    fn explore_viewports(&mut self, event: &ui_events::ExploreViewports) {
        if self.explore.is_shown() {
            self.explore.viewports(event);
        }
    }

    fn restore_explore_positions(&mut self, event: &ExplorePositionsRestored) {
        self.explore.restore_positions(&event.0);
    }
}

/// Input: keys, shortcuts, pointer and paste.
impl DiffComponent {
    fn keyboard_input(&mut self, input: PaneInput) -> Vec<Action> {
        match input {
            PaneInput::Conversation(command) => self.conversation_command(command),
            PaneInput::Viewer(input) => self.route(|viewer| viewer.keyboard_input(input)),
        }
    }

    fn run_global_shortcut(&mut self, command: DiffGlobalShortcut) -> Vec<Action> {
        if self.conversation_shown() {
            return Vec::new();
        }
        self.route(|viewer| viewer.run_global_shortcut(command))
    }

    fn pointer_input(&mut self, mut input: PointerInput) -> Vec<Action> {
        if self.explore.is_shown() {
            return self.document_viewer_mut().pointer_input(input);
        }
        if let Some(peek) = &mut self.conversation.peek {
            if let Some(position) = &mut input.position {
                if position.component_row == 0 {
                    if matches!(input.kind, PointerInputKind::Click) {
                        self.close_peek();
                    }
                    return Vec::new();
                }
                position.component_row = position.component_row.saturating_sub(1);
            }
            return peek.viewer_mut().pointer_input(input);
        }
        if self.conversation.active {
            return self.conversation_pointer(input);
        }
        self.files.pointer_input(input)
    }

    fn paste(&mut self, input: &TextPasted) -> Vec<Action> {
        if self.conversation_shown() && self.conversation_editor_visible() {
            return self.conversation_paste(input);
        }
        self.route(|viewer| viewer.paste(input))
    }
}

/// Subscribe one viewer handler through a routing policy of the pane.
macro_rules! subscribe_viewer {
    ($subscriptions:ident, $policy:ident, $handler:path) => {
        $subscriptions.subscribe(|pane: &mut DiffComponent, event| {
            pane.handle(event, |pane, event| {
                pane.$policy(|viewer| $handler(viewer, event))
            })
        })
    };
}

impl Component<Action> for DiffComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        // The pane's own handlers.
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::navigation_changed));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::conversation_selected));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::restore_explore_positions));
        subscriptions
            .subscribe(|pane, event| pane.handle(event, Self::explore_comparison_accepted));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::explore_evidence));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::explore_viewports));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::explore_evidence_input));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::comment_paste));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::repository_changed));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::viewport_changed));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::source_content_loaded));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::source_content_failed));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::refresh_current_file));
        // Asynchronous results reach every viewer.
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::replies_displayed));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::threads_loaded));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::post_finished));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::highlighting_finished));
        subscriptions.subscribe(|pane, event| pane.handle(event, Self::search_completed));
        // Events about the shown documents reach the document viewer.
        subscribe_viewer!(subscriptions, for_document, SourceViewer::file_selected);
        subscribe_viewer!(subscriptions, for_document, SourceViewer::animation_tick);
        subscribe_viewer!(subscriptions, for_document, SourceViewer::content_loaded);
        subscribe_viewer!(
            subscriptions,
            for_document,
            SourceViewer::content_load_failed
        );
        subscribe_viewer!(
            subscriptions,
            for_document,
            SourceViewer::reviewable_files_changed
        );
        subscribe_viewer!(
            subscriptions,
            for_document,
            SourceViewer::review_state_saved
        );
        subscribe_viewer!(
            subscriptions,
            for_document,
            SourceViewer::target_jump_requested
        );
        subscribe_viewer!(
            subscriptions,
            for_document,
            SourceViewer::revision_edit_failed
        );
        // Requests from the reviewer reach the viewer they look at.
        subscribe_viewer!(subscriptions, route, SourceViewer::clear_input);
        subscribe_viewer!(subscriptions, route, SourceViewer::preview_source_location);
        subscribe_viewer!(
            subscriptions,
            route,
            SourceViewer::location_list_visibility_changed
        );
        subscribe_viewer!(subscriptions, route, SourceViewer::accept_source_location);
        subscribe_viewer!(subscriptions, route, SourceViewer::restore_review_location);
        subscribe_viewer!(subscriptions, route, SourceViewer::location_jumped);
        subscriptions.subscribe_input(
            InputScope::Focused,
            component_core::AnyInput,
            |pane, input: TextPasted| pane.handle(&input, Self::paste),
        );
        subscriptions.subscribe_input(InputScope::Focused, PaneKeyMatcher::new(), |pane, input| {
            pane.handle(input, Self::keyboard_input)
        });
        subscriptions.subscribe_input(InputScope::Global, ShortcutMatcher::new(), |pane, input| {
            pane.handle(input, Self::run_global_shortcut)
        });
        subscriptions.subscribe_input(
            InputScope::Hovered,
            component_core::AnyInput,
            |pane, input| pane.handle(input, Self::pointer_input),
        );
    }
}

/// What one key does in the pane.
#[derive(Clone, Copy)]
enum PaneInput {
    Conversation(ConversationCommand),
    Viewer(ViewerInput),
}

struct PaneKeyMatcher {
    shortcuts: ShortcutMatcher<DiffPaneCommand>,
    conversation: ShortcutMatcher<ConversationCommand>,
}

impl PaneKeyMatcher {
    const fn new() -> Self {
        Self {
            shortcuts: ShortcutMatcher::new(),
            conversation: ShortcutMatcher::new(),
        }
    }
}

impl InputMatcher<DiffComponent, Key> for PaneKeyMatcher {
    type Output = PaneInput;

    fn resolve(&mut self, pane: &DiffComponent, key: &Key) -> InputResolution<Self::Output> {
        let key = *key;
        let peeking = pane.visible_peek().is_some();
        if peeking && ConversationView::goes_back(key) && !pane.input_viewer().search.is_editing() {
            return InputResolution::Matched(PaneInput::Conversation(
                ConversationCommand::Conversation(ConversationShortcut::Back),
            ));
        }
        // Switching navigation reaches the application even while the comment
        // editor or the search prompt takes every other key.
        if ApplicationShortcut::bound_to(key) == Some(ApplicationShortcut::ToggleNavigation) {
            return InputResolution::NoMatch;
        }
        if pane.conversation_shown() {
            if pane.conversation_editor_visible() {
                return InputResolution::Matched(PaneInput::Viewer(ViewerInput::CommentKey(key)));
            }
            return self
                .conversation
                .resolve_key(key)
                .map(PaneInput::Conversation);
        }
        pane.input_viewer()
            .resolve_key(key, &mut self.shortcuts)
            .map(PaneInput::Viewer)
    }
}
