//! Diff document state and its component event boundary.
//!
//! A [`SourceViewer`] shows documents for one view: the Files diff, one Explore
//! evidence block, or a thread's source peek. The [`DiffComponent`] pane owns
//! every viewer and decides which one receives each event.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use component_core::EventPublisher;
use diff_rendering::FrameOverlay;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use review_source::ReviewCheckpoint;
use review_thread_projection::SharedThreadProjection;
use ui_actions::{
    Action, DocumentAction, DocumentLoad, LspAction, RepositoryAction, TerminalAction,
};
use ui_events::{
    AnimationTick, CurrentReviewLocationChanged, DiffContentLoadFailed, DiffContentLoaded,
    DiffInputClearRequested, DiffTargetJumpRequested, DiffViewportChanged,
    DisplayedDiffViewportsChanged, FileDecorationsChanged, FileSelected, FileSelectionRequested,
    FileSummary, HighlightRequest, HighlightingFinished, LocationListVisibilityChanged,
    PointerInput, PointerInputKind, RepositoryFilesChanged, ReviewLocation, ReviewLocationJumped,
    ReviewLocationRestoreRequested, ReviewStateSaved, ReviewableFiles, ReviewableFilesChanged,
    RevisionEditFailed, SourceContentLoadFailed, SourceContentLoaded, SourceLocationAccepted,
    SourceLocationPreviewRequested, TemporaryFilesChanged, ToastRequested,
};
use ui_shortcuts::{
    DiffGlobalShortcut, DiffPaneCommand, DiffShortcut, HunkShortcut, Key, LocationShortcut,
    LspShortcut, MovementShortcut, SearchMatchShortcut, SearchShortcut, SourceShortcut,
};

mod clipped_viewport;
mod comment_layout;
mod comments;
mod context;
mod conversation;
mod document;
mod embedded;
mod evidence_source;
mod explore;
mod explore_restore;
mod history;
mod hunk_marks;
mod pane;
mod presentation;
mod render;
mod reply_visibility;
mod title_controls;

pub use clipped_viewport::ClippedViewport;
use diff_position::Position;
use diff_search::{Direction as SearchDirection, Edit as SearchEdit, Intent as SearchIntent};
use diff_search::{Location as SearchLocation, Search};
use document::LoadedDocument;
use history::{LocationHistory, LocationHistoryDirection};
pub use pane::DiffComponent;
use presentation::{DiffPresentation, PresentedRow};
use render::{DiffPointerViewport, DiffRenderer, DiffViewport, TAB_DISPLAY_WIDTH};
use std::ops::RangeInclusive;
pub use syntax_highlighting::SyntaxHighlighter;
use syntax_highlighting::Token;
use title_controls::TitleControls;
use ui_theme::Palette;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DiffPointerPosition {
    row: usize,
    column: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffControl {
    ExpandAll,
    ContractAll,
    ShowFile,
    CloseFile,
}

#[derive(Clone, Copy)]
enum ScrollPlacement {
    KeepCursorVisible,
    CenterCursor,
    AlignTargetTop,
}

/// What a viewer shows, fixed when the pane creates it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Role {
    /// The reviewed diff. It alone reports its location, viewports and file
    /// decorations to the rest of the application.
    Files,
    /// One Explore evidence block, showing a saved comparison.
    Evidence,
    /// A read-only view of the current file behind a review thread.
    Peek,
}

impl Role {
    /// Whether the viewer speaks for the reviewed diff in application events.
    fn publishes_review(self) -> bool {
        self == Self::Files
    }

    /// Whether the viewer shows review threads and opens comment editors.
    fn shows_comments(self) -> bool {
        self != Self::Peek
    }

    /// Whether the viewer shows Explore evidence and loads sources from it.
    fn explores(self) -> bool {
        self == Self::Evidence
    }
}

/// The application services every viewer of one pane shares.
#[derive(Clone)]
struct Services {
    events: EventPublisher,
    highlighter: SyntaxHighlighter,
    repository_root: PathBuf,
    palette: Palette,
    drafts: std::rc::Rc<RefCell<review_drafts::Drafts>>,
    thread_projection: SharedThreadProjection,
}

/// Documents, position, selection, search, highlighting and comment layout
/// for one view of source code.
pub struct SourceViewer {
    role: Role,
    evidence: explore::ShownEvidence,
    events: EventPublisher,
    reply_visibility: RefCell<reply_visibility::ReplyVisibility>,
    /// The snapshot this viewer's source loads use instead of the checkpoint.
    source_session: Option<String>,
    comments: comments::Comments,
    /// Where the viewer reports thread placement.
    thread_projection: SharedThreadProjection,
    reviewable_files: ReviewableFiles,
    review_checkpoint: Option<ReviewCheckpoint>,
    documents: Vec<LoadedDocument>,
    selected_path: Option<String>,
    pending_load_path: Option<String>,
    preview: Option<LoadedDocument>,
    pending_preview_location: Option<review_lsp::SourceLocation>,
    pending_center_path: Option<String>,
    pending_target_jump: Option<review_source::DiffTarget>,
    search: Search,
    selection: Option<SelectionState>,
    viewport_width: u16,
    viewport_height: u16,
    drag_anchor: Option<usize>,
    highlighter: SyntaxHighlighter,
    repository_root: PathBuf,
    palette: Palette,
    location_history: LocationHistory,
    pending_history_navigation: Option<PendingHistoryNavigation>,
    rendered_pointer_viewport: RefCell<Option<DiffPointerViewport>>,
    /// Paths whose hunk mark is being saved and reloaded.
    pending_hunk_marks: HashSet<String>,
}

#[derive(Clone, Debug)]
struct PendingHistoryNavigation {
    target: ReviewLocation,
    previous_history: LocationHistory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SelectionState {
    anchor: usize,
    cursor: usize,
    fixed: bool,
}

impl SelectionState {
    fn range(self) -> RangeInclusive<usize> {
        self.anchor.min(self.cursor)..=self.anchor.max(self.cursor)
    }
}

impl SourceViewer {
    /// Create an empty viewer with the pane's shared services.
    fn new(services: &Services, role: Role, reviewable_files: ReviewableFiles) -> Self {
        Self {
            role,
            events: services.events.clone(),
            source_session: None,
            evidence: explore::ShownEvidence::default(),
            comments: comments::Comments::new(
                std::rc::Rc::clone(&services.drafts),
                services.thread_projection.clone(),
            ),
            thread_projection: services.thread_projection.clone(),
            reviewable_files,
            review_checkpoint: None,
            documents: Vec::new(),
            selected_path: None,
            pending_load_path: None,
            preview: None,
            pending_preview_location: None,
            pending_center_path: None,
            pending_target_jump: None,
            search: Search::default(),
            selection: None,
            viewport_width: 80,
            viewport_height: 24,
            drag_anchor: None,
            highlighter: services.highlighter.clone(),
            repository_root: services.repository_root.clone(),
            palette: services.palette,
            location_history: LocationHistory::default(),
            pending_history_navigation: None,
            rendered_pointer_viewport: RefCell::new(None),
            reply_visibility: RefCell::default(),
            pending_hunk_marks: HashSet::new(),
        }
    }

    /// File selection defers loading to the next application tick.
    fn has_pending_load(&self) -> bool {
        self.pending_load_path.is_some()
    }

    /// The snapshot this viewer loads sources from.
    fn source_snapshot(&self) -> Option<&str> {
        self.source_session.as_deref().or_else(|| {
            self.review_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.checkpoint.as_str())
        })
    }

    /// Draw the selected diff document and return its positioned frame layer.
    fn render(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        palette: Palette,
        focused: bool,
    ) -> FrameOverlay {
        let result = self.renderer(palette, focused).render(
            area,
            buffer,
            &mut self.reply_visibility.borrow_mut(),
        );
        self.rendered_pointer_viewport
            .replace(result.pointer_viewport);
        result.frame_overlay
    }

    /// Map one pointer position through the same visual layout as rendering.
    fn pointer_position(
        &self,
        width: u16,
        palette: Palette,
        focused: bool,
        screen_row: usize,
        pane_column: usize,
    ) -> Option<DiffPointerPosition> {
        let cached_position = self
            .rendered_pointer_viewport
            .borrow()
            .as_ref()
            .and_then(|viewport| viewport.position(screen_row, pane_column));
        let (source_row, source_column) = if let Some(position) = cached_position {
            position
        } else {
            let file = self.displayed_document()?;
            let viewport = self
                .renderer(palette, focused)
                .viewport(file, width, focused);
            let visual_row = viewport
                .top(file.document.position())
                .saturating_add(screen_row);
            (
                viewport.source_row_at(visual_row)?,
                viewport.source_column_at(
                    visual_row,
                    pane_column,
                    file.document.diff.line_number_width(),
                ),
            )
        };
        Some(DiffPointerPosition {
            row: source_row,
            column: source_column,
        })
    }

    /// Resolve one pane-title control at a pane-relative column.
    fn control_at(&self, width: u16, column: u16) -> Option<DiffControl> {
        TitleControls::shown(self.selected_document()?, width)?.at(width, column)
    }

    fn selected_document(&self) -> Option<&LoadedDocument> {
        let path = self.selected_path.as_deref()?;
        self.documents.iter().find(|document| document.path == path)
    }

    fn selected_document_mut(&mut self) -> Option<&mut LoadedDocument> {
        let path = self.selected_path.as_deref()?;
        self.documents
            .iter_mut()
            .find(|document| document.path == path)
    }

    fn displayed_document(&self) -> Option<&LoadedDocument> {
        self.preview.as_ref().or_else(|| self.selected_document())
    }

    fn displayed_document_mut(&mut self) -> Option<&mut LoadedDocument> {
        if self.preview.is_some() {
            self.preview.as_mut()
        } else {
            self.selected_document_mut()
        }
    }

    /// Resize the viewer. Returns whether its size changed.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn viewport_changed(&mut self, event: &DiffViewportChanged) -> bool {
        let changed = self.viewport_width != event.width.max(1)
            || self.viewport_height != event.height.max(1);
        self.viewport_width = event.width.max(1);
        self.viewport_height = event.height.max(1);
        self.comments.fit_editors(self.viewport_height);
        if changed && !self.comments.in_conversation() {
            if self.editor_is_visible() {
                self.keep_comment_visible();
            } else {
                self.keep_cursor_visible();
            }
        }
        changed
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn clear_input(&mut self, _event: &DiffInputClearRequested) -> Vec<Action> {
        self.selection = None;
        let intents = self.search.clear();
        self.apply_search(intents)
    }

    /// Decide what `key` does in this viewer: edit its open comment, type
    /// into its search prompt, or run a diff shortcut.
    fn resolve_key(
        &self,
        key: Key,
        shortcuts: &mut ui_shortcuts::ShortcutMatcher<DiffPaneCommand>,
    ) -> component_core::InputResolution<ViewerInput> {
        if self.editor_is_visible() {
            return component_core::InputResolution::Matched(ViewerInput::CommentKey(key));
        }
        if self.search.is_editing() {
            return component_core::InputResolution::Matched(ViewerInput::SearchKey(key));
        }
        shortcuts.resolve_key(key).map(ViewerInput::Shortcut)
    }

    fn keyboard_input(&mut self, input: ViewerInput) -> Vec<Action> {
        match input {
            ViewerInput::CommentKey(key) => self.comment_key(key),
            ViewerInput::SearchKey(key) => self.edit_search(key),
            ViewerInput::Shortcut(command) => self.run_shortcut(command),
        }
    }

    fn pointer_input(&mut self, input: PointerInput) -> Vec<Action> {
        if let Some(actions) = self.comment_pointer_input(input) {
            return actions;
        }
        let pointer_position = input.position.and_then(|position| {
            if position.component_row == 0 {
                None
            } else {
                self.pointer_position(
                    self.viewport_width,
                    self.palette,
                    true,
                    usize::from(position.component_row.saturating_sub(1)),
                    usize::from(position.component_column.saturating_sub(1)),
                )
            }
        });
        match input.kind {
            PointerInputKind::Scroll(delta) => self.scroll(delta),
            PointerInputKind::Click => {
                if let Some(row) = self.hunk_control_at(&input) {
                    return self.toggle_hunk_review(row);
                }
                self.click(&input, pointer_position);
            }
            PointerInputKind::Drag => self.extend_pointer_selection(pointer_position),
            PointerInputKind::Release => {
                self.drag_anchor = None;
                self.finish_pointer_selection();
                self.publish_viewports();
                return Vec::new();
            }
            PointerInputKind::ControlClick
            | PointerInputKind::DoubleClick
            | PointerInputKind::RightClick => {}
        }
        self.publish_viewports();
        Vec::new()
    }

    /// A click on a pane-title control runs it; elsewhere it starts a selection.
    fn click(&mut self, input: &PointerInput, pointer_position: Option<DiffPointerPosition>) {
        if let Some(position) = input.position
            && position.component_row == 0
            && let Some(control) = self.control_at(
                self.viewport_width.saturating_add(2),
                position.component_column,
            )
        {
            self.activate_diff_control(control);
        } else {
            self.start_pointer_selection(pointer_position);
        }
    }

    fn finish_pointer_selection(&mut self) {
        if self.role.shows_comments() && self.selection.is_some() {
            self.add_comment();
        }
    }

    fn start_pointer_selection(&mut self, position: Option<DiffPointerPosition>) {
        let Some(position) = position else {
            return;
        };
        self.selection = None;
        if !self.comments.in_conversation() {
            self.comments.park_editor();
        }
        if self.expand_context(position.row) {
            self.drag_anchor = None;
            self.publish_current_location();
            return;
        }
        self.position_cursor(position);
        self.drag_anchor = Some(position.row);
    }

    fn extend_pointer_selection(&mut self, position: Option<DiffPointerPosition>) {
        let Some(position) = position else {
            return;
        };
        let anchor = *self.drag_anchor.get_or_insert(position.row);
        self.position_cursor(position);
        self.selection = Some(SelectionState {
            anchor,
            cursor: position.row,
            fixed: false,
        });
    }

    fn activate_diff_control(&mut self, control: DiffControl) {
        match control {
            DiffControl::ExpandAll => {
                self.change_context(DiffPresentation::expand_all);
            }
            DiffControl::ContractAll => {
                self.change_context(DiffPresentation::contract_all);
            }
            DiffControl::ShowFile | DiffControl::CloseFile => {
                if let Some(document) = self.selected_document_mut() {
                    if control == DiffControl::ShowFile {
                        let _ = document.document.diff.show_file();
                    } else {
                        let _ = document.document.diff.show_diff();
                    }
                }
                self.keep_cursor_visible();
            }
        }
        self.publish_current_location();
    }

    fn position_cursor(&mut self, position: DiffPointerPosition) {
        let Some(document) = self.displayed_document_mut() else {
            return;
        };
        document.document.place_cursor(position);
        self.keep_cursor_visible();
        self.publish_current_location();
    }

    fn scroll(&mut self, delta: isize) {
        self.move_screen(|position, rows, height| position.scroll_by(delta, rows, height));
    }

    fn displayed_viewport(&self) -> Option<DiffViewport> {
        let document = self.displayed_document()?;
        Some(
            self.renderer(self.palette, true)
                .viewport(document, self.viewport_width, true),
        )
    }

    fn contain_displayed_cursor(&mut self) {
        self.move_screen(|position, rows, height| position.contain_cursor(rows, height));
    }

    /// Apply one screen operation to the displayed document's position. When
    /// it moves the cursor onto the screen, the selection follows, the new
    /// location is published, and the cursor is contained once more in the
    /// layout for its new place: moving an end-of-line cursor can remove a
    /// wrapped row.
    fn move_screen(
        &mut self,
        operation: impl FnOnce(&mut Position, &render::DocumentRows<'_>, usize) -> bool,
    ) {
        if self.apply_screen_operation(operation) {
            self.apply_screen_operation(|position, rows, height| {
                position.contain_cursor(rows, height)
            });
        }
    }

    /// Apply one screen operation to the displayed document's position.
    /// Returns whether it moved the cursor, after following that move with
    /// the selection and publishing the new location.
    fn apply_screen_operation(
        &mut self,
        operation: impl FnOnce(&mut Position, &render::DocumentRows<'_>, usize) -> bool,
    ) -> bool {
        let height = usize::from(self.viewport_height);
        let Some(viewport) = self.displayed_viewport() else {
            return false;
        };
        let document = self.displayed_document_mut().expect("the document exists");
        let (position, rows) = document.document.on_screen(&viewport);
        if !operation(position, &rows, height) {
            return false;
        }
        let row = position.cursor();
        document.document.clear_source_location();
        if let Some(selection) = &mut self.selection
            && !selection.fixed
        {
            selection.cursor = row;
        }
        self.publish_current_location();
        self.publish_search_status();
        true
    }

    fn run_shortcut(&mut self, command: DiffPaneCommand) -> Vec<Action> {
        let comment = matches!(command, DiffPaneCommand::Diff(DiffShortcut::Comment(_)));
        if self.ignores_shortcut(comment) {
            return Vec::new();
        }
        match command {
            DiffPaneCommand::Movement(command) => self.run_movement_shortcut(command),
            DiffPaneCommand::Search(command) => self.search(command),
            DiffPaneCommand::Diff(command) => self.run_diff_shortcut(command),
        }
    }

    fn run_global_shortcut(&mut self, command: DiffGlobalShortcut) -> Vec<Action> {
        let comment = matches!(
            command,
            DiffGlobalShortcut::PreviousComment | DiffGlobalShortcut::NextComment
        );
        if self.ignores_shortcut(comment) {
            return Vec::new();
        }
        match command {
            DiffGlobalShortcut::PreviousComment => self.navigate_comment(false),
            DiffGlobalShortcut::NextComment => self.navigate_comment(true),
            DiffGlobalShortcut::ToggleHunkReviewed => {
                let cursor = self
                    .selected_document()
                    .map(|document| document.document.position().cursor());
                return cursor.map_or_else(Vec::new, |row| self.toggle_hunk_review(row));
            }
            DiffGlobalShortcut::Hunk(command) => self.navigate_modified_hunk(command),
            DiffGlobalShortcut::OpenInEditor => return self.open_in_editor(),
        }
        Vec::new()
    }

    /// Whether a shortcut must not act on the shown document: comment
    /// shortcuts do nothing in a read-only viewer.
    fn ignores_shortcut(&self, comment: bool) -> bool {
        comment && !self.role.shows_comments()
    }

    fn run_diff_shortcut(&mut self, command: DiffShortcut) -> Vec<Action> {
        match command {
            DiffShortcut::Comment(command) => self.comment_shortcut(command),
            DiffShortcut::Location(LocationShortcut::GoToPrevious) => {
                self.navigate_location_history(LocationHistoryDirection::Previous)
            }
            DiffShortcut::Location(LocationShortcut::GoToNext) => {
                self.navigate_location_history(LocationHistoryDirection::Next)
            }
            DiffShortcut::SearchMatch(command) => self.search_match(command),
            DiffShortcut::Source(command) => self.source(command),
            DiffShortcut::Lsp(LspShortcut::Restart) => vec![Action::Lsp(LspAction::Restart)],
            DiffShortcut::Lsp(command) => self.lsp(command),
            DiffShortcut::StartSelection => {
                self.start_selection();
                Vec::new()
            }
        }
    }

    fn navigate_modified_hunk(&mut self, command: HunkShortcut) {
        let Some(document) = self.selected_document() else {
            return;
        };
        let current_row = document.document.position().cursor();
        let modified_hunk_rows = document.document.diff.modified_hunk_rows();
        let target = match command {
            HunkShortcut::GoToNextModified => modified_hunk_rows
                .iter()
                .copied()
                .find(|row| *row > current_row)
                .or_else(|| modified_hunk_rows.first().copied()),
            HunkShortcut::GoToPreviousModified => modified_hunk_rows
                .iter()
                .rev()
                .copied()
                .find(|row| *row < current_row)
                .or_else(|| modified_hunk_rows.last().copied()),
        };
        if let Some(target) = target {
            self.jump_cursor(target);
        }
    }

    fn run_movement_shortcut(&mut self, command: MovementShortcut) -> Vec<Action> {
        let origin = matches!(
            command,
            MovementShortcut::GoToFirst | MovementShortcut::GoToLast
        )
        .then(|| self.current_review_location())
        .flatten();
        self.navigate(command);
        self.record_current_location_jump(origin);
        Vec::new()
    }

    fn navigate(&mut self, command: MovementShortcut) {
        let Some(document) = self.selected_document() else {
            return;
        };
        let current_row = document.document.position().cursor();
        let half_page =
            isize::try_from(usize::from(self.viewport_height) / 2).unwrap_or(isize::MAX);
        let target_row = match command {
            MovementShortcut::MoveDown => current_row.saturating_add(1),
            MovementShortcut::MoveUp => current_row.saturating_sub(1),
            MovementShortcut::GoToFirst => 0,
            MovementShortcut::GoToLast => document.document.diff.len().saturating_sub(1),
            MovementShortcut::MoveHalfPageDown => return self.navigate_visual_rows(half_page),
            MovementShortcut::MoveHalfPageUp => return self.navigate_visual_rows(-half_page),
        };
        if matches!(
            command,
            MovementShortcut::GoToFirst | MovementShortcut::GoToLast
        ) {
            self.jump_cursor(target_row);
        } else {
            self.move_cursor(target_row);
        }
    }

    fn navigate_visual_rows(&mut self, delta: isize) {
        let target = self.selected_document().and_then(|document| {
            self.renderer(self.palette, true)
                .viewport(document, self.viewport_width, true)
                .source_position_after_visual_delta(document, delta)
        });
        let Some((row, column)) = target else {
            return;
        };
        self.move_cursor(row);
        if let Some(document) = self.selected_document_mut()
            && let Some((_, line)) = document.document.diff.source_position(row)
        {
            document
                .document
                .set_column(display_column_to_byte(&line, column));
        }
        self.keep_cursor_visible();
    }

    fn move_cursor(&mut self, target: usize) {
        self.set_cursor(target);
        self.keep_cursor_visible();
        self.publish_viewports();
        self.publish_current_location();
        self.publish_search_status();
    }

    fn jump_cursor(&mut self, target: usize) {
        self.jump_cursor_to_column(target, None);
    }

    fn jump_cursor_to_column(&mut self, target: usize, column: Option<usize>) {
        self.set_cursor(target);
        if let (Some(column), Some(document)) = (column, self.selected_document_mut()) {
            document.document.set_column(column);
        }
        self.center_jump_target();
        self.publish_viewports();
        self.publish_current_location();
        self.publish_search_status();
    }

    fn set_cursor(&mut self, target: usize) {
        let mut cursor = target;
        if let Some(document) = self.selected_document_mut() {
            document.document.move_cursor(target);
            document.document.clear_source_location();
            cursor = document.document.position().cursor();
        }
        if let Some(selection) = &mut self.selection
            && !selection.fixed
        {
            selection.cursor = cursor;
        }
    }

    fn keep_cursor_visible(&mut self) {
        self.place_scroll(ScrollPlacement::KeepCursorVisible);
    }

    fn center_jump_target(&mut self) {
        self.place_scroll(ScrollPlacement::CenterCursor);
    }

    fn align_target_jump_to_viewport_top(&mut self) {
        self.place_scroll(ScrollPlacement::AlignTargetTop);
    }

    fn place_scroll(&mut self, placement: ScrollPlacement) {
        let height = usize::from(self.viewport_height);
        let Some(viewport) = self.displayed_viewport() else {
            return;
        };
        let document = self.displayed_document_mut().expect("the document exists");
        let (position, rows) = document.document.on_screen(&viewport);
        match placement {
            ScrollPlacement::KeepCursorVisible => position.keep_cursor_visible(&rows, height),
            ScrollPlacement::CenterCursor => position.center_cursor(&rows, height),
            ScrollPlacement::AlignTargetTop => position.align_cursor_top(&rows),
        }
    }

    fn start_selection(&mut self) {
        let Some(cursor) = self
            .selected_document()
            .map(|document| document.document.position().cursor())
        else {
            return;
        };
        if !self
            .selected_document()
            .is_some_and(|document| document.document.diff.is_selectable(cursor))
        {
            return;
        }
        let finalized = self.selection.is_some_and(|selection| !selection.fixed);
        self.selection = match self.selection {
            Some(mut selection) if !selection.fixed => {
                selection.fixed = true;
                Some(selection)
            }
            _ => Some(SelectionState {
                anchor: cursor,
                cursor,
                fixed: false,
            }),
        };
        if finalized {
            self.add_comment();
        }
    }

    fn search(&mut self, command: SearchShortcut) -> Vec<Action> {
        match command {
            SearchShortcut::Begin => self.begin_search(None),
        }
    }

    fn search_match(&mut self, command: SearchMatchShortcut) -> Vec<Action> {
        let direction = match command {
            SearchMatchShortcut::WordUnderCursor => {
                return if let Some(word) = self.word_under_cursor() {
                    self.begin_search(Some(word))
                } else {
                    self.apply_search(vec![SearchIntent::Status { files: true }])
                };
            }
            SearchMatchShortcut::NextMatch => SearchDirection::Forward,
            SearchMatchShortcut::PreviousMatch => SearchDirection::Backward,
        };
        let intents = self
            .search
            .step(direction, self.current_search_location().as_ref());
        self.apply_search(intents)
    }

    /// Carry out what the search asks for, in order.
    fn apply_search(&mut self, intents: Vec<SearchIntent>) -> Vec<Action> {
        let mut actions = Vec::new();
        for intent in intents {
            match intent {
                SearchIntent::Jump { to, remember } => {
                    let origin = remember.then(|| self.current_review_location()).flatten();
                    self.jump_to_search_location(&to);
                    self.record_current_location_jump(origin);
                }
                SearchIntent::Request(request) => {
                    actions.push(Action::Document(DocumentAction::Search(Some(request))));
                }
                SearchIntent::CancelRequest => {
                    actions.push(Action::Document(DocumentAction::Search(None)));
                }
                SearchIntent::Status { files } => {
                    if files {
                        self.publish_decorations();
                    }
                    self.publish_search_status();
                }
            }
        }
        actions
    }

    /// Type a new query, or find `word` when one is given.
    fn begin_search(&mut self, word: Option<String>) -> Vec<Action> {
        self.selection = None;
        let origin = self.search_cursor_location();
        let intents = match word {
            Some(word) => self.search.find_word(word, origin, &self.documents),
            None => self.search.begin_typing(origin, &self.documents),
        };
        let mut actions = self.apply_search(intents);
        if let Some(action) = self.load_all_diffs_for_search() {
            self.search.wait_for_repository();
            actions.push(action);
        }
        actions
    }

    fn edit_search(&mut self, key: Key) -> Vec<Action> {
        let edit = match key {
            Key::Char(character) => SearchEdit::Type(character),
            Key::Backspace => SearchEdit::Backspace,
            Key::Enter => SearchEdit::Accept,
            Key::Escape => SearchEdit::Cancel,
            _ => return Vec::new(),
        };
        let intents = self.search.edit(edit, &self.documents);
        self.apply_search(intents)
    }

    fn paste(&mut self, input: &ui_events::TextPasted) -> Vec<Action> {
        if self.editor_is_visible() {
            return self.comment_paste(input);
        }
        let intents = self
            .search
            .edit(SearchEdit::Paste(&input.0), &self.documents);
        self.apply_search(intents)
    }

    fn load_all_diffs_for_search(&mut self) -> Option<Action> {
        let review_checkpoint = self.review_checkpoint.clone()?;
        let paths = self
            .documents
            .iter_mut()
            .filter_map(LoadedDocument::start_diff_load)
            .collect::<Vec<_>>();
        (!paths.is_empty()).then_some(Action::Document(DocumentAction::Load(
            DocumentLoad::Diffs {
                review_checkpoint,
                paths,
            },
        )))
    }

    fn refresh_search_matches(&mut self) -> Vec<Action> {
        let intents = self.search.refresh(&self.documents);
        self.apply_search(intents)
    }

    /// Accept background search results while the reviewer looks at this viewer.
    fn complete_search(&mut self, results: &text_search::Results) -> Vec<Action> {
        let intents = self.search.complete(results, &self.documents);
        self.apply_search(intents)
    }

    /// Keep background search results while this viewer is hidden.
    fn retain_search_results(&mut self, results: &text_search::Results) {
        self.search.retain(results, &self.documents);
    }

    fn jump_to_search_location(&mut self, location: &SearchLocation) {
        self.selected_path = Some(location.path.clone());
        self.jump_cursor_to_column(location.position.row, Some(location.position.column));
        self.publish_file_selection(location.path.clone());
    }

    fn word_under_cursor(&self) -> Option<String> {
        let document = self.selected_document()?;
        let (_, line) = document
            .document
            .diff
            .source_position(document.document.position().cursor())?;
        let mut column = document.document.position().column().min(line.len());
        while !line.is_char_boundary(column) {
            column = column.saturating_sub(1);
        }
        let start = line[..column]
            .char_indices()
            .rev()
            .find(|(_, character)| !is_word_character(*character))
            .map_or(0, |(index, character)| index + character.len_utf8());
        let end = line[column..]
            .char_indices()
            .find(|(_, character)| !is_word_character(*character))
            .map_or(line.len(), |(index, _)| column + index);
        (start < end).then(|| line[start..end].to_owned())
    }

    fn source(&mut self, command: SourceShortcut) -> Vec<Action> {
        let Some(document) = self.selected_document() else {
            return Vec::new();
        };
        let row = document.document.position().cursor();
        if command == SourceShortcut::ExpandOrMoveRight && self.expand_context(row) {
            self.publish_viewports();
            self.publish_current_location();
            return Vec::new();
        }
        let document = self.selected_document_mut().expect("the document exists");
        let source_line = document.document.diff.source_text(row);
        let Some(source_line) = source_line.as_deref() else {
            return Vec::new();
        };
        let column = document.document.position().column();
        let column = match command {
            SourceShortcut::MoveLeft => previous_character_column(source_line, column),
            SourceShortcut::ExpandOrMoveRight => next_character_column(source_line, column),
            SourceShortcut::MoveToStartOfLine => 0,
            SourceShortcut::MoveToEndOfLine => source_line.len(),
            SourceShortcut::MoveToNextWord => next_word_column(source_line, column),
            SourceShortcut::MoveToPreviousWord => previous_word_column(source_line, column),
        };
        document.document.set_column(column);
        self.keep_cursor_visible();
        self.publish_viewports();
        self.publish_current_location();
        Vec::new()
    }

    fn open_in_editor(&self) -> Vec<Action> {
        let Some(document) = self.displayed_document() else {
            return Vec::new();
        };
        let line = document
            .document
            .diff
            .source_position(document.document.position().cursor())
            .map(|(line, _)| line);
        vec![Action::Terminal(TerminalAction::OpenInEditor {
            path: self.document_disk_path(document),
            line,
        })]
    }

    fn document_disk_path(&self, document: &LoadedDocument) -> PathBuf {
        document.disk_path.clone().unwrap_or_else(|| {
            let path = PathBuf::from(&document.path);
            if path.is_absolute() {
                path
            } else {
                self.repository_root.join(path)
            }
        })
    }

    fn lsp(&self, command: LspShortcut) -> Vec<Action> {
        if self.role.explores() && !self.explore_lsp_ready() {
            return Vec::new();
        }
        let operation = match command {
            LspShortcut::ShowDocumentation => review_lsp::Operation::Hover,
            LspShortcut::GoToDefinition => review_lsp::Operation::Definition,
            LspShortcut::GoToTypeDefinition => review_lsp::Operation::TypeDefinition,
            LspShortcut::GoToReferences => review_lsp::Operation::References,
            LspShortcut::Restart => return vec![Action::Lsp(LspAction::Restart)],
        };
        let (Some(document), Some(snapshot_id)) =
            (self.selected_document(), self.source_snapshot())
        else {
            return Vec::new();
        };
        let Some((line, expected_line)) = document
            .document
            .diff
            .source_position(document.document.position().cursor())
        else {
            return Vec::new();
        };
        let mut byte_column = document
            .document
            .position()
            .column()
            .min(expected_line.len());
        while !expected_line.is_char_boundary(byte_column) {
            byte_column = byte_column.saturating_sub(1);
        }
        let path = self.document_disk_path(document);
        vec![Action::Lsp(LspAction::Request {
            operation,
            query: review_lsp::Query {
                toast_id: toasts::ToastId::generate(),
                path,
                line,
                byte_column,
                expected_line,
                snapshot_id: snapshot_id.to_owned(),
            },
        })]
    }

    /// The rows the reviewer selected in the shown document.
    fn selected_rows(&self) -> Option<RangeInclusive<usize>> {
        self.selection.map(SelectionState::range)
    }

    fn renderer(&self, palette: Palette, focused: bool) -> DiffRenderer<'_> {
        DiffRenderer::new(
            palette,
            self.displayed_document(),
            focused,
            self.role.explores()
                || self
                    .selected_path
                    .as_deref()
                    .is_some_and(|path| self.reviewable_files.contains(path)),
            self.search.query(),
            self.selected_rows(),
        )
        .with_comments(&self.comments)
        .with_evidence(&self.evidence)
    }

    /// Whether `event` keeps the review this viewer shows.
    fn shows_review_of(&self, event: &RepositoryFilesChanged) -> bool {
        self.shows_review_unit(&event.review_checkpoint.review_unit)
    }

    fn shows_review_unit(&self, unit: &review_types::ReviewUnit) -> bool {
        self.review_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| &checkpoint.review_unit == unit)
    }

    fn repository_changed(&mut self, event: &RepositoryFilesChanged) -> Vec<Action> {
        self.comments.forget_unavailable();
        self.preview = None;
        self.pending_preview_location = None;
        let same_review_unit = self.shows_review_of(event);
        let same_checkpoint = self
            .review_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint == &event.review_checkpoint);
        self.prepare_review_switch(same_review_unit);
        if !same_review_unit {
            self.pending_center_path = None;
        }
        if !same_checkpoint {
            self.pending_target_jump = None;
        }
        let mut previous_documents = std::mem::take(&mut self.documents);
        self.documents = event
            .files
            .iter()
            .map(|summary| {
                let path = summary.path();
                let previous_document = previous_documents
                    .iter()
                    .position(|document| document.path == path)
                    .map(|index| previous_documents.swap_remove(index));
                let mut document = LoadedDocument::from_summary(summary);
                if same_review_unit && let Some(previous_document) = previous_document {
                    let preserve_content =
                        same_checkpoint || self.selected_path.as_deref() == Some(path.as_str());
                    document.preserve_state_from(
                        &previous_document,
                        preserve_content,
                        same_checkpoint,
                    );
                }
                document
            })
            .collect();
        if same_review_unit {
            self.documents.extend(
                previous_documents
                    .into_iter()
                    .filter(|file| file.comments_only),
            );
        }
        self.review_checkpoint = Some(event.review_checkpoint.clone());
        self.refresh_comment_documents();
        let thread_load = (!same_review_unit).then(|| {
            Action::Thread(review_threads::ThreadCommand::Load(
                event.review_checkpoint.review_unit.clone(),
            ))
        });
        self.pending_load_path = None;
        let mut search_action = self.refresh_search_matches();
        search_action.extend(thread_load);
        self.publish_viewports();
        self.publish_decorations();
        self.publish_current_location();
        self.publish_search_status();
        if self
            .pending_history_navigation
            .as_ref()
            .is_some_and(|pending| {
                pending.target.review_unit() == &event.review_checkpoint.review_unit
            })
            && let Some(pending) = self.pending_history_navigation.take()
        {
            let mut actions = self.restore_location(&pending.target);
            actions.extend(search_action);
            return actions;
        }
        if self.search.is_active() {
            let action = self.load_all_diffs_for_search();
            if action.is_some() {
                self.search.wait_for_repository();
            }
            action.into_iter().chain(search_action).collect()
        } else {
            self.selected_load_action()
                .into_iter()
                .chain(search_action)
                .collect()
        }
    }

    fn file_selected(&mut self, event: &FileSelected) -> Vec<Action> {
        if self.role.explores() {
            return Vec::new();
        }
        let newly_selected = self.selected_path.as_deref() != Some(&event.path);
        if self
            .pending_target_jump
            .as_ref()
            .is_some_and(|target| target.path() != event.path)
        {
            self.pending_target_jump = None;
        }
        let origin = self.current_review_location();
        let selected_is_repository_file = self
            .documents
            .iter()
            .any(|document| document.path == event.path && !document.temporary);
        if selected_is_repository_file && self.documents.iter().any(|document| document.temporary) {
            self.documents.retain(|document| !document.temporary);
            self.events.publish(TemporaryFilesChanged::default());
        }
        let load_immediately = self.selected_path.is_none();
        self.selected_path = Some(event.path.clone());
        if newly_selected && !self.comments.in_conversation() {
            self.comments.park_editor_outside(&event.path);
        }
        if newly_selected {
            self.contain_displayed_cursor();
        }
        self.publish_viewports();
        self.publish_current_location();
        self.publish_search_status();
        self.record_current_location_jump(origin);
        let mut actions = if load_immediately {
            self.selected_load_action().into_iter().collect()
        } else {
            self.pending_load_path = Some(event.path.clone());
            Vec::new()
        };
        if newly_selected {
            let path = self
                .selected_document()
                .and_then(|document| document.disk_path.clone())
                .unwrap_or_else(|| self.repository_root.join(&event.path));
            actions.push(Action::Lsp(LspAction::OpenDocument(path)));
        }
        actions.extend(self.request_visible_highlights());
        actions
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn animation_tick(&mut self, _event: &AnimationTick) -> Vec<Action> {
        let Some(path) = self.pending_load_path.take() else {
            return Vec::new();
        };
        if self.selected_path.as_deref() != Some(path.as_str()) {
            return Vec::new();
        }
        self.selected_load_action().into_iter().collect()
    }

    fn selected_load_action(&mut self) -> Option<Action> {
        let review_checkpoint = self.review_checkpoint.clone()?;
        let selected_path = self.selected_path.as_deref()?;
        if !self.reviewable_files.contains(selected_path) {
            return None;
        }
        let document = self
            .documents
            .iter_mut()
            .find(|document| document.path == selected_path)?;
        document.start_diff_load().map(|path| {
            Action::Document(DocumentAction::Load(DocumentLoad::Diff {
                review_checkpoint,
                path,
            }))
        })
    }

    fn content_loaded(&mut self, event: &DiffContentLoaded) -> Vec<Action> {
        if self.role.explores() {
            return Vec::new();
        }
        let syntax_highlighter = self.highlighter.clone();
        let Some(document) = self.current_document_mut(&event.review_checkpoint, &event.path)
        else {
            return Vec::new();
        };
        let highlighted_rows = syntax_highlighter.plain(
            event.rows.clone(),
            event.old_content.as_deref(),
            event.new_content.as_deref(),
        );
        let reload_after_current_load = document
            .replace_diff(DiffPresentation::new(highlighted_rows).with_hunks(event.hunks.clone()));
        let content = Arc::new(event.clone());
        document.content = Some(Arc::clone(&content));
        let request = HighlightRequest::Diff(content);
        document.document.prepare_highlighting(request);
        self.hunk_marks_reloaded(&event.path);
        self.comments.refresh_anchors(&self.documents);
        self.refresh_pending_preview(&event.path);
        let pending_center_completed = self.pending_center_path.as_deref() == Some(&event.path);
        let center_selected_document =
            pending_center_completed && self.selected_path.as_deref() == Some(&event.path);
        if pending_center_completed {
            self.pending_center_path = None;
        }
        if center_selected_document {
            self.center_jump_target();
        }
        if !reload_after_current_load {
            self.finish_pending_target_jump(&event.path);
        }
        let intents = self.search.documents_loaded(&self.documents);
        let search_action = self.apply_search(intents);
        self.finish_repository_search_load_if_complete();
        self.contain_loaded_cursor(&event.path);
        if self.comments.take_pending_path(&event.path) {
            self.keep_comment_visible();
        }
        self.publish_viewports();
        self.publish_decorations();
        self.publish_current_location();
        self.publish_search_status();
        let highlights = self.request_visible_highlights();
        if reload_after_current_load && self.selected_path.as_deref() == Some(event.path.as_str()) {
            return self
                .selected_load_action()
                .into_iter()
                .chain(search_action)
                .chain(highlights)
                .collect();
        }
        search_action.into_iter().chain(highlights).collect()
    }

    fn highlighting_finished(&mut self, event: &HighlightingFinished) {
        for loaded in self.documents.iter_mut().chain(self.preview.iter_mut()) {
            loaded.document.finish_highlighting(event);
        }
    }

    fn contain_loaded_cursor(&mut self, path: &str) {
        if self
            .displayed_document()
            .is_some_and(|document| document.path == path)
        {
            self.contain_displayed_cursor();
        }
    }

    fn request_visible_highlights(&mut self) -> Vec<Action> {
        self.documents
            .iter_mut()
            .filter(|loaded| Some(&loaded.path) == self.selected_path.as_ref())
            .chain(self.preview.iter_mut())
            .filter_map(|loaded| loaded.document.request_highlighting())
            .collect()
    }

    fn content_load_failed(&mut self, event: &DiffContentLoadFailed) -> Vec<Action> {
        self.hunk_marks_reloaded(&event.path);
        if self.pending_center_path.as_deref() == Some(&event.path) {
            self.pending_center_path = None;
        }
        let pending_target_jump_matches = self
            .pending_target_jump
            .as_ref()
            .is_some_and(|target| target.path() == event.path);
        let reload_immediately = self
            .current_document_mut(&event.review_checkpoint, &event.path)
            .is_some_and(LoadedDocument::fail_diff_load);
        if pending_target_jump_matches && !reload_immediately {
            self.pending_target_jump = None;
        }
        self.finish_repository_search_load_if_complete();
        if reload_immediately && self.selected_path.as_deref() == Some(event.path.as_str()) {
            return self.selected_load_action().into_iter().collect();
        }
        Vec::new()
    }

    fn finish_repository_search_load_if_complete(&mut self) {
        let repository_is_loading = self.documents.iter().any(LoadedDocument::is_loading);
        if !repository_is_loading {
            self.search.repository_loaded();
        }
    }

    fn current_document_mut(
        &mut self,
        review_checkpoint: &ReviewCheckpoint,
        path: &str,
    ) -> Option<&mut LoadedDocument> {
        if self.review_checkpoint.as_ref() != Some(review_checkpoint) {
            return None;
        }
        self.documents
            .iter_mut()
            .find(|document| document.path == path)
    }

    fn target_jump_requested(&mut self, event: &DiffTargetJumpRequested) -> Vec<Action> {
        let origin = self.current_review_location();
        let Some(document) = self.documents.get(event.file_index) else {
            return Vec::new();
        };
        let path = document.path.clone();
        self.selected_path = Some(path.clone());
        if let Some(document) = self.selected_document_mut() {
            document.document.set_column(0);
        }
        let row = event.row.or_else(|| {
            self.selected_document().and_then(|document| {
                let viewport = document.target_viewport(event.file_index);
                diff_rendering::target_rows(&viewport, &event.target).map(|rows| rows.0)
            })
        });
        if let Some(row) = row {
            self.set_cursor(row);
        }
        self.align_target_jump_to_viewport_top();
        let load_was_active = self
            .selected_document()
            .is_some_and(LoadedDocument::is_loading);
        let load_action = self.selected_load_action();
        if row.is_none() || load_was_active || load_action.is_some() {
            self.pending_target_jump = Some(event.target.clone());
        } else {
            self.pending_target_jump = None;
        }
        self.publish_file_selection(path);
        self.publish_viewports();
        self.publish_current_location();
        self.record_current_location_jump(origin);
        load_action.into_iter().collect()
    }

    fn finish_pending_target_jump(&mut self, loaded_path: &str) {
        let Some(pending) = self.pending_target_jump.take() else {
            return;
        };
        if pending.path() != loaded_path || self.selected_path.as_deref() != Some(loaded_path) {
            if pending.path() != loaded_path {
                self.pending_target_jump = Some(pending);
            }
            return;
        }
        let Some(file_index) = self
            .documents
            .iter()
            .position(|document| document.path == loaded_path)
        else {
            return;
        };
        let row = self.documents.get(file_index).and_then(|document| {
            let viewport = document.target_viewport(file_index);
            diff_rendering::target_rows(&viewport, &pending).map(|rows| rows.0)
        });
        if let Some(row) = row {
            self.set_cursor(row);
            self.align_target_jump_to_viewport_top();
        }
    }

    fn preview_source_location(&mut self, event: &SourceLocationPreviewRequested) -> Vec<Action> {
        self.preview = None;
        self.pending_preview_location = Some(event.location.clone());
        if self.role.explores() {
            return self.explore_source(event.location.clone(), ui_events::SourceLoadMode::Preview);
        }
        let Some(review_checkpoint) = self.review_checkpoint.clone() else {
            return Vec::new();
        };
        if let Some(path) = event.location.review_path(&self.repository_root)
            && let Some(document) = self
                .documents
                .iter_mut()
                .find(|document| document.path == path)
        {
            document.disk_path = Some(event.location.path.clone());
            let mut preview = document.clone();
            if preview.document.reveal_location(&event.location) {
                self.preview = Some(preview);
                self.center_jump_target();
                return Vec::new();
            }
            return document
                .start_diff_load()
                .map(|path| {
                    Action::Document(DocumentAction::Load(DocumentLoad::Diff {
                        review_checkpoint,
                        path,
                    }))
                })
                .into_iter()
                .collect();
        }
        vec![Action::Document(DocumentAction::Load(
            DocumentLoad::Source {
                snapshot_id: self
                    .source_snapshot()
                    .expect("checkpoint exists")
                    .to_owned(),
                location: event.location.clone(),
                mode: ui_actions::SourceLoadMode::Preview,
            },
        ))]
    }

    fn accept_source_location(&mut self, event: &SourceLocationAccepted) -> Vec<Action> {
        self.preview = None;
        self.pending_preview_location = None;
        let origin = self.current_review_location();
        if let (Some(origin), Some(review_checkpoint)) = (&origin, &self.review_checkpoint) {
            self.location_history.record(
                origin.clone(),
                &ReviewLocation::Source {
                    review_unit: review_checkpoint.review_unit.clone(),
                    location: event.location.clone(),
                },
            );
        }
        self.load_source_location(event.location.clone(), ui_actions::SourceLoadMode::External)
    }

    fn load_source_location(
        &mut self,
        location: review_lsp::SourceLocation,
        mode: ui_actions::SourceLoadMode,
    ) -> Vec<Action> {
        if self.role.explores() {
            return self.explore_source(location, mode);
        }
        if self.review_checkpoint.is_none() {
            return Vec::new();
        }
        if let Some(path) = location.review_path(&self.repository_root)
            && let Some(document) = self
                .documents
                .iter_mut()
                .find(|document| document.path == path)
        {
            document.disk_path = Some(location.path.clone());
            let revealed = document.document.reveal_location(&location);
            if mode.is_external() {
                self.selected_path = Some(path.clone());
                if revealed {
                    self.center_jump_target();
                }
                self.publish_file_selection(path.clone());
                self.publish_viewports();
                self.publish_current_location();
            }
            let action = self.selected_load_action();
            if mode.is_external() && !revealed {
                self.pending_center_path = Some(path);
            }
            return action.into_iter().collect();
        }
        vec![Action::Document(DocumentAction::Load(
            DocumentLoad::Source {
                snapshot_id: self
                    .source_snapshot()
                    .expect("checkpoint exists")
                    .to_owned(),
                location,
                mode,
            },
        ))]
    }

    fn source_content_loaded(&mut self, event: &SourceContentLoaded) -> Vec<Action> {
        if self.source_snapshot() != Some(event.snapshot_id.as_str()) {
            return Vec::new();
        }
        let review_path = event.location.review_path(&self.repository_root);
        let highlighted = self
            .highlighter
            .plain(Vec::new(), None, Some(&event.content));
        let mut presentation = DiffPresentation::new(highlighted);
        let _ = presentation.show_file();
        let mut document = LoadedDocument::from_source(
            &event.location,
            review_path.as_deref(),
            presentation,
            event.mode,
        );
        if self.role.explores() {
            self.evidence.identify_source(
                &mut document,
                &event.location.path,
                &self.repository_root,
            );
        }
        let request = HighlightRequest::Source(Arc::new(event.clone()));
        document.document.prepare_highlighting(request);
        let _ = document.document.reveal_location(&event.location);
        if event.mode.is_external() {
            self.selection = None;
            self.documents.retain(|document| !document.temporary);
            let path = document.path.clone();
            self.documents.push(document);
            self.selected_path = Some(path.clone());
            self.center_jump_target();
            if self.role.publishes_review() {
                self.events.publish(TemporaryFilesChanged {
                    files: vec![FileSummary::temporary(
                        path.clone(),
                        event.location.path.display().to_string(),
                        event.location.path.clone(),
                    )],
                });
            }
            self.publish_file_selection(path);
            self.publish_viewports();
            self.publish_current_location();
        } else if self.pending_preview_location.as_ref() == Some(&event.location) {
            self.preview = Some(document);
            self.center_jump_target();
        } else {
            return Vec::new();
        }
        let mut actions = self.request_visible_highlights();
        if event.mode.is_external() {
            actions.push(Action::Lsp(LspAction::OpenDocument(
                event.location.path.clone(),
            )));
        }
        actions
    }

    fn refresh_pending_preview(&mut self, path: &str) {
        let Some(location) = self.pending_preview_location.clone() else {
            return;
        };
        if location.review_path(&self.repository_root).as_deref() != Some(path) {
            return;
        }
        let Some(document) = self.documents.iter().find(|document| document.path == path) else {
            return;
        };
        let mut preview = document.clone();
        preview.disk_path = Some(location.path.clone());
        if preview.document.reveal_location(&location) {
            self.preview = Some(preview);
            self.center_jump_target();
        }
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn location_list_visibility_changed(&mut self, event: &LocationListVisibilityChanged) {
        if !event.visible {
            self.preview = None;
            self.pending_preview_location = None;
        }
    }

    fn source_content_failed(&self, event: &SourceContentLoadFailed) {
        if self.source_snapshot() == Some(event.snapshot_id.as_str()) {
            self.events.publish(ToastRequested {
                text: event.message.clone(),
                kind: toasts::ToastKind::Error,
            });
        }
    }

    fn restore_review_location(&mut self, event: &ReviewLocationRestoreRequested) -> Vec<Action> {
        self.restore_location(&event.location)
    }

    fn restore_location(&mut self, location: &ReviewLocation) -> Vec<Action> {
        match location {
            ReviewLocation::LoadedDocument {
                review_unit,
                path,
                cursor,
                presentation_location,
                column,
            } if self
                .review_checkpoint
                .as_ref()
                .is_some_and(|checkpoint| &checkpoint.review_unit == review_unit) =>
            {
                let Some(document) = self
                    .documents
                    .iter_mut()
                    .find(|document| &document.path == path)
                else {
                    return Vec::new();
                };
                document.document.restore_presentation_location(
                    *presentation_location,
                    *cursor,
                    *column,
                );
                self.selected_path = Some(path.clone());
                self.center_jump_target();
                self.publish_file_selection(path.clone());
                self.publish_viewports();
                self.publish_current_location();
                let load_already_active = self
                    .selected_document()
                    .is_some_and(LoadedDocument::is_loading);
                let action = self.selected_load_action();
                if load_already_active || action.is_some() {
                    self.pending_center_path = Some(path.clone());
                }
                action.into_iter().collect()
            }
            ReviewLocation::Source { location, .. } => {
                self.load_source_location(location.clone(), ui_actions::SourceLoadMode::External)
            }
            ReviewLocation::Revision { .. } | ReviewLocation::LoadedDocument { .. } => Vec::new(),
        }
    }

    fn location_jumped(&mut self, event: &ReviewLocationJumped) {
        self.location_history
            .record(event.origin.clone(), &event.target);
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn revision_edit_failed(&mut self, _event: &RevisionEditFailed) {
        if let Some(pending) = self.pending_history_navigation.take() {
            self.location_history = pending.previous_history;
        }
    }

    fn record_current_location_jump(&mut self, origin: Option<ReviewLocation>) {
        if let (Some(origin), Some(target)) = (origin, self.current_review_location()) {
            self.location_history.record(origin, &target);
        }
    }

    fn navigate_location_history(&mut self, direction: LocationHistoryDirection) -> Vec<Action> {
        let Some(current) = self.current_review_location() else {
            return Vec::new();
        };
        let current_review_unit = current.review_unit().clone();
        let reviewable_files = self.reviewable_files.clone();
        let repository_root = self.repository_root.clone();
        let previous_history = self.location_history.clone();
        let source_only = !self.role.publishes_review();
        let Some(target) = self
            .location_history
            .navigate(direction, current, |location| {
                if source_only || location.review_unit() != &current_review_unit {
                    return true;
                }
                match location {
                    ReviewLocation::LoadedDocument { path, .. } => reviewable_files.contains(path),
                    ReviewLocation::Source { location, .. } => location
                        .review_path(&repository_root)
                        .is_none_or(|path| reviewable_files.contains(&path)),
                    ReviewLocation::Revision { .. } => true,
                }
            })
        else {
            return Vec::new();
        };
        if target.review_unit() != &current_review_unit {
            let change_id = review_repository::repository::ChangeId::from(target.review_unit());
            self.pending_history_navigation = Some(PendingHistoryNavigation {
                target,
                previous_history,
            });
            return vec![Action::Repository(RepositoryAction::EditRevision {
                change_id,
            })];
        }
        self.events
            .publish(ReviewLocationRestoreRequested { location: target });
        Vec::new()
    }

    fn current_review_location(&self) -> Option<ReviewLocation> {
        let review_unit = self.review_checkpoint.as_ref()?.review_unit.clone();
        let Some(document) = self.selected_document() else {
            return Some(ReviewLocation::Revision { review_unit });
        };
        if document.temporary {
            return document
                .cursor_location()
                .map(|location| ReviewLocation::Source {
                    review_unit,
                    location,
                });
        }
        Some(ReviewLocation::LoadedDocument {
            review_unit,
            path: document.path.clone(),
            cursor: document.document.position().cursor(),
            presentation_location: document
                .document
                .diff
                .presentation_location(document.document.position().cursor()),
            column: document.document.position().column(),
        })
    }

    fn publish_file_selection(&self, path: String) {
        if self.role.publishes_review() {
            self.events.publish(FileSelectionRequested { path });
        }
    }

    fn publish_current_location(&self) {
        if !self.role.publishes_review() {
            return;
        }
        self.events.publish(CurrentReviewLocationChanged {
            location: self.current_review_location(),
        });
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn reviewable_files_changed(&mut self, _event: &ReviewableFilesChanged) -> Vec<Action> {
        self.scroll(0);
        self.publish_viewports();
        self.selected_load_action().into_iter().collect()
    }

    fn review_state_saved(&mut self, event: &ReviewStateSaved) -> Vec<Action> {
        let is_current_review_unit = self
            .review_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.review_unit == event.review_unit);
        // Every state that still needs review shows a diff from a new version.
        let needs_review = event
            .result
            .as_ref()
            .is_ok_and(|state| state.status.needs_review());
        let reloading = is_current_review_unit
            && needs_review
            && self
                .documents
                .iter()
                .any(|document| document.path == event.path);
        self.hunk_mark_saved(&event.path, reloading);
        let Some(document) = self
            .documents
            .iter_mut()
            .find(|document| document.path == event.path)
            .filter(|_| reloading)
        else {
            return Vec::new();
        };
        document.require_diff_reload();
        if self.selected_path.as_deref() != Some(event.path.as_str()) {
            return Vec::new();
        }
        self.selected_load_action().into_iter().collect()
    }

    fn publish_viewports(&self) {
        if !self.role.publishes_review() {
            return;
        }
        self.place_threads();
        let viewports = self
            .documents
            .iter()
            .enumerate()
            .filter(|(_, document)| self.reviewable_files.contains(&document.path))
            .map(|(file_index, document)| document.target_viewport(file_index))
            .collect();
        let current_file_index = self
            .selected_path
            .as_deref()
            .and_then(|path| {
                self.documents
                    .iter()
                    .position(|document| document.path == path)
            })
            .unwrap_or(0);
        let current_row = self
            .documents
            .get(current_file_index)
            .map_or(0, LoadedDocument::presented_row);
        self.events.publish(DisplayedDiffViewportsChanged {
            viewports,
            current_file_index,
            current_row,
        });
    }

    fn publish_decorations(&self) {
        if !self.role.publishes_review() {
            return;
        }
        let notice_paths = self
            .documents
            .iter()
            .filter(|document| document.has_notice())
            .map(|document| document.path.clone())
            .collect();
        let search_match_paths = self.search.match_paths();
        self.events.publish(FileDecorationsChanged {
            notice_paths,
            search_match_paths,
        });
    }

    fn publish_search_status(&self) {
        let current_location = self.current_search_location();
        self.events
            .publish(self.search.status(current_location.as_ref()));
    }

    fn search_cursor_location(&self) -> SearchLocation {
        self.current_search_location()
            .unwrap_or_else(|| SearchLocation {
                document_index: 0,
                path: self.selected_path.clone().unwrap_or_default(),
                position: text_search::Position { row: 0, column: 0 },
            })
    }

    fn current_search_location(&self) -> Option<SearchLocation> {
        let (document_index, document) =
            self.documents.iter().enumerate().find(|(_, document)| {
                Some(document.path.as_str()) == self.selected_path.as_deref()
            })?;
        Some(SearchLocation {
            document_index,
            path: document.path.clone(),
            position: text_search::Position {
                row: document.document.position().cursor(),
                column: document.document.position().column(),
            },
        })
    }
}

/// What one key does in a viewer.
#[derive(Clone, Copy)]
enum ViewerInput {
    CommentKey(Key),
    SearchKey(Key),
    Shortcut(DiffPaneCommand),
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn character_boundary_at_or_before(line: &str, column: usize) -> usize {
    let mut column = column.min(line.len());
    while !line.is_char_boundary(column) {
        column = column.saturating_sub(1);
    }
    column
}

fn previous_character_column(line: &str, column: usize) -> usize {
    let current = character_boundary_at_or_before(line, column);
    line[..current]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_character_column(line: &str, column: usize) -> usize {
    let current = character_boundary_at_or_before(line, column);
    line[current..]
        .char_indices()
        .nth(1)
        .map_or(line.len(), |(index, _)| current + index)
}

fn next_word_column(line: &str, column: usize) -> usize {
    let characters = line.char_indices().collect::<Vec<_>>();
    let mut index = characters.partition_point(|(byte, _)| *byte < column.min(line.len()));
    if characters
        .get(index)
        .is_some_and(|(_, character)| is_word_character(*character))
    {
        while characters
            .get(index)
            .is_some_and(|(_, character)| is_word_character(*character))
        {
            index += 1;
        }
    }
    while characters
        .get(index)
        .is_some_and(|(_, character)| !is_word_character(*character))
    {
        index += 1;
    }
    characters.get(index).map_or(line.len(), |(byte, _)| *byte)
}

fn previous_word_column(line: &str, column: usize) -> usize {
    let characters = line.char_indices().collect::<Vec<_>>();
    let mut index = characters.partition_point(|(byte, _)| *byte < column.min(line.len()));
    while index > 0 && !is_word_character(characters[index - 1].1) {
        index -= 1;
    }
    while index > 0 && is_word_character(characters[index - 1].1) {
        index -= 1;
    }
    characters.get(index).map_or(0, |(byte, _)| *byte)
}

fn display_column_to_byte(line: &str, display_column: usize) -> usize {
    let mut display = 0_usize;
    for (byte, grapheme) in line.grapheme_indices(true) {
        let width = if grapheme == "\t" {
            TAB_DISPLAY_WIDTH
        } else {
            grapheme.width()
        };
        if display.saturating_add(width) > display_column {
            return byte;
        }
        display = display.saturating_add(width);
    }
    line.len()
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
