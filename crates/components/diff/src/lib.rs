//! Diff document state and its component event boundary.

use std::cell::RefCell;
use std::path::PathBuf;

use component_core::{
    Component, ComponentSubscriptions, EventPublisher, InputMatcher, InputResolution, InputScope,
};
use guide_rendering::{GuideLayout, GuideOverlay};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use review_guide::ReviewCheckpoint;
use ui_actions::Action;
use ui_events::{
    AnimationTick, CurrentReviewLocationChanged, DiffContentLoadFailed, DiffContentLoaded,
    DiffInputClearRequested, DiffViewportChanged, DisplayedDiffViewportsChanged,
    FileDecorationsChanged, FileSelected, FileSelectionRequested, FileSummary, GuideJumpRequested,
    GuideLayoutChanged, LocationListVisibilityChanged, OutputDeliveryFinished, OutputTargetChanged,
    PointerInput, PointerInputKind, RepositoryFilesChanged, ReviewLocation, ReviewLocationJumped,
    ReviewLocationRestoreRequested, ReviewableFiles, ReviewableFilesChanged, RevisionEditFailed,
    SearchStatusChanged, SourceContentLoadFailed, SourceContentLoaded, SourceLocationAccepted,
    SourceLocationPreviewRequested, TemporaryFilesChanged, ToastRequested,
};
use ui_shortcuts::{
    ApplicationShortcut, Key, LspShortcut, NavigationShortcut, SearchShortcut, ShortcutCommand,
    ShortcutMatcher, ShortcutSet, SourceShortcut,
};

mod document;
mod history;
mod presentation;
mod render;

use document::LoadedDocument;
use history::{LocationHistory, LocationHistoryDirection};
use presentation::{DiffPresentation, PresentedRow, SearchDirection, SearchMatch, matching_ranges};
use render::{DiffPointerViewport, DiffRenderer, DiffViewport, TAB_DISPLAY_WIDTH};
use std::ops::RangeInclusive;
pub use syntax_highlighting::SyntaxHighlighter;
use syntax_highlighting::Token;
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
    AlignGuideTop,
}

/// Build the visual row mapping used by diff input and refresh handling.
fn layout_diff_viewport(
    file: &LoadedDocument,
    width: u16,
    palette: Palette,
    search_query: Option<&str>,
    selection: Option<RangeInclusive<usize>>,
    focused: bool,
    guide_layout: Option<GuideLayout<'_>>,
) -> DiffViewport {
    DiffRenderer::new(
        palette,
        Some(file),
        guide_layout,
        focused,
        true,
        search_query,
        selection,
    )
    .viewport(file, width, focused)
}

/// Loaded diff documents and their guide-facing viewport projection.
pub struct DiffComponent {
    events: EventPublisher,
    reviewable_files: ReviewableFiles,
    review_checkpoint: Option<ReviewCheckpoint>,
    documents: Vec<LoadedDocument>,
    selected_path: Option<String>,
    pending_load_path: Option<String>,
    preview: Option<LoadedDocument>,
    pending_preview_location: Option<review_lsp::SourceLocation>,
    pending_center_path: Option<String>,
    search: Option<SearchState>,
    selection: Option<SelectionState>,
    viewport_width: u16,
    viewport_height: u16,
    drag_anchor: Option<usize>,
    highlighter: SyntaxHighlighter,
    output_target: review_store::OutputTarget,
    repository_root: PathBuf,
    palette: Palette,
    location_history: LocationHistory,
    pending_history_navigation: Option<PendingHistoryNavigation>,
    guide_items: Vec<review_guide::GuideItem>,
    guide_counters: Vec<Option<ui_events::GuideCounter>>,
    rendered_pointer_viewport: RefCell<Option<DiffPointerViewport>>,
}

#[derive(Clone, Debug)]
struct PendingHistoryNavigation {
    target: ReviewLocation,
    previous_history: LocationHistory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SearchState {
    query: String,
    origin: RepositorySearchLocation,
    editing: bool,
    waiting_for_repository: bool,
    matches: Vec<RepositorySearchLocation>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RepositorySearchLocation {
    document_index: usize,
    path: String,
    position: SearchMatch,
}

impl SearchState {
    fn replace_matches(&mut self, documents: &[LoadedDocument]) {
        if self.query.is_empty() {
            self.matches.clear();
            return;
        }
        self.matches = documents
            .iter()
            .enumerate()
            .flat_map(|(document_index, document)| {
                document
                    .document
                    .diff
                    .matching_positions(&self.query)
                    .into_iter()
                    .map(move |position| RepositorySearchLocation {
                        document_index,
                        path: document.path.clone(),
                        position,
                    })
            })
            .collect();
    }

    fn first_after(
        &self,
        reference: &RepositorySearchLocation,
    ) -> Option<&RepositorySearchLocation> {
        self.matches
            .iter()
            .find(|location| location > &reference)
            .or_else(|| self.matches.first())
    }

    fn first_before(
        &self,
        reference: &RepositorySearchLocation,
    ) -> Option<&RepositorySearchLocation> {
        self.matches
            .iter()
            .rev()
            .find(|location| location < &reference)
            .or_else(|| self.matches.last())
    }
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

impl DiffComponent {
    /// Create an empty diff component.
    pub fn new(
        events: EventPublisher,
        reviewable_files: ReviewableFiles,
        highlighter: SyntaxHighlighter,
        output_target: review_store::OutputTarget,
        repository_root: PathBuf,
        palette: Palette,
    ) -> Self {
        Self {
            events,
            reviewable_files,
            review_checkpoint: None,
            documents: Vec::new(),
            selected_path: None,
            pending_load_path: None,
            preview: None,
            pending_preview_location: None,
            pending_center_path: None,
            search: None,
            selection: None,
            viewport_width: 80,
            viewport_height: 24,
            drag_anchor: None,
            highlighter,
            output_target,
            repository_root,
            palette,
            location_history: LocationHistory::default(),
            pending_history_navigation: None,
            guide_items: Vec::new(),
            guide_counters: Vec::new(),
            rendered_pointer_viewport: RefCell::new(None),
        }
    }

    /// Draw the selected diff document and return its positioned guide layer.
    pub fn render(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        palette: Palette,
        focused: bool,
        guide_layout: Option<GuideLayout<'_>>,
    ) -> GuideOverlay {
        let result = self
            .renderer(palette, guide_layout, focused)
            .render(area, buffer);
        self.rendered_pointer_viewport
            .replace(result.pointer_viewport);
        result.guide_overlay
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
                .renderer(palette, None, focused)
                .viewport(file, width, focused);
            let visual_row = viewport.scroll(file).saturating_add(screen_row);
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
        const EXPAND: &str = "[←→]";
        const CONTRACT: &str = "[→←]";
        const SHOW_FILE: &str = "[👁 ]";
        let file = self.selected_document()?;
        let labels: &[(&str, DiffControl)] = if file.document.diff.is_file_view() {
            &[("[x]", DiffControl::CloseFile)]
        } else if file.document.diff.can_show_file() {
            &[
                (EXPAND, DiffControl::ExpandAll),
                (CONTRACT, DiffControl::ContractAll),
                (SHOW_FILE, DiffControl::ShowFile),
            ]
        } else {
            &[
                (EXPAND, DiffControl::ExpandAll),
                (CONTRACT, DiffControl::ContractAll),
            ]
        };
        let title_width = labels.iter().map(|(label, _)| label.width()).sum::<usize>()
            + labels.len().saturating_sub(1);
        let start = usize::from(width).saturating_sub(title_width + 1);
        let offset = usize::from(column).checked_sub(start)?;
        let mut label_start = 0;
        for (label, control) in labels {
            let label_end = label_start + label.width();
            if (label_start..label_end).contains(&offset) {
                return Some(*control);
            }
            label_start = label_end + 1;
        }
        None
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

    fn guide_layout<'a>(&'a self, document: &LoadedDocument) -> Option<GuideLayout<'a>> {
        if self.preview.is_some() {
            return None;
        }
        let file_index = self
            .documents
            .iter()
            .position(|candidate| candidate.path == document.path)?;
        let viewport = document.guide_viewport(file_index);
        Some(GuideLayout::new(
            &self.guide_items,
            &self.guide_counters,
            viewport.rows.len(),
            self.palette.guide,
            |target| guide_rendering::target_rows(&viewport, target),
        ))
    }

    /// Return the displayed guide projection for frame composition.
    pub fn displayed_guide_viewport(&self) -> Option<ui_events::DisplayedDiffViewport> {
        if self.preview.is_some() {
            return None;
        }
        let document = self.displayed_document()?;
        let file_index = self
            .documents
            .iter()
            .position(|candidate| candidate.path == document.path)
            .unwrap_or_default();
        Some(document.guide_viewport(file_index))
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn viewport_changed(&mut self, event: &DiffViewportChanged) {
        let changed = self.viewport_width != event.width.max(1)
            || self.viewport_height != event.height.max(1);
        self.viewport_width = event.width.max(1);
        self.viewport_height = event.height.max(1);
        if changed {
            self.keep_cursor_visible();
        }
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn clear_input(&mut self, _event: &DiffInputClearRequested) {
        self.search = None;
        self.selection = None;
        self.publish_decorations();
        self.publish_search_status();
    }

    fn keyboard_input(&mut self, input: DiffKeyboardInput) -> Vec<Action> {
        match input {
            DiffKeyboardInput::SearchKey(key) => {
                self.edit_search(key);
                Vec::new()
            }
            DiffKeyboardInput::Shortcut(command) => self.run_shortcut(command),
        }
    }

    fn pointer_input(&mut self, input: PointerInput) -> Vec<Action> {
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
            PointerInputKind::Click { .. } => {
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
            PointerInputKind::Drag => self.extend_pointer_selection(pointer_position),
            PointerInputKind::Release => {
                self.drag_anchor = None;
                let actions = self.insert_selection();
                self.publish_viewports();
                return actions;
            }
            PointerInputKind::ControlClick
            | PointerInputKind::DoubleClick
            | PointerInputKind::RightClick => {}
        }
        self.publish_viewports();
        Vec::new()
    }

    fn start_pointer_selection(&mut self, position: Option<DiffPointerPosition>) {
        let Some(position) = position else {
            return;
        };
        self.drag_anchor = Some(position.row);
        self.position_cursor(position);
        self.selection = None;
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
        if let Some(document) = self.selected_document_mut() {
            match control {
                DiffControl::ExpandAll => {
                    let _ = document.document.diff.expand_all();
                }
                DiffControl::ContractAll => {
                    let _ = document.document.diff.contract_all();
                }
                DiffControl::ShowFile => {
                    let _ = document.document.diff.show_file();
                }
                DiffControl::CloseFile => {
                    let _ = document.document.diff.show_diff();
                }
            }
        }
        self.keep_cursor_visible();
    }

    fn position_cursor(&mut self, position: DiffPointerPosition) {
        let Some(document) = self.displayed_document_mut() else {
            return;
        };
        document.document.cursor = position
            .row
            .min(document.document.diff.len().saturating_sub(1));
        if let Some(display_column) = position.column
            && let Some((_, line)) = document
                .document
                .diff
                .source_position(document.document.cursor)
        {
            document.document.column = display_column_to_byte(&line, display_column);
        }
        document.document.clear_source_location();
        self.keep_cursor_visible();
        self.publish_current_location();
    }

    fn scroll(&mut self, delta: isize) {
        let height = usize::from(self.viewport_height);
        let maximum = self.displayed_document().map_or(0, |document| {
            let guide_layout = self.guide_layout(document);
            layout_diff_viewport(
                document,
                self.viewport_width,
                self.palette,
                self.search_query(),
                self.selection.map(SelectionState::range),
                true,
                guide_layout,
            )
            .visible_row_count()
            .saturating_sub(height)
        });
        let Some(document) = self.displayed_document_mut() else {
            return;
        };
        document.document.scroll = document
            .document
            .scroll
            .saturating_add_signed(delta)
            .min(maximum);
    }

    fn run_shortcut(&mut self, command: ShortcutCommand) -> Vec<Action> {
        match command {
            ShortcutCommand::Navigation(command) => self.run_navigation_shortcut(command),
            ShortcutCommand::Search(command) => self.search(command),
            ShortcutCommand::Source(command) => self.source(command),
            ShortcutCommand::Application(command) => self.run_application_shortcut(command),
            ShortcutCommand::Lsp(LspShortcut::Restart) => vec![Action::RestartLsp],
            ShortcutCommand::Lsp(command) => self.lsp(command),
            ShortcutCommand::Guide(_) => Vec::new(),
        }
    }

    fn run_navigation_shortcut(&mut self, command: NavigationShortcut) -> Vec<Action> {
        match command {
            NavigationShortcut::GoToPreviousLocation => {
                self.navigate_location_history(LocationHistoryDirection::Previous)
            }
            NavigationShortcut::GoToNextLocation => {
                self.navigate_location_history(LocationHistoryDirection::Next)
            }
            command => {
                let origin = matches!(
                    command,
                    NavigationShortcut::GoToFirst | NavigationShortcut::GoToLast
                )
                .then(|| self.current_review_location())
                .flatten();
                self.navigate(command);
                self.record_current_location_jump(origin);
                Vec::new()
            }
        }
    }

    fn run_application_shortcut(&mut self, command: ApplicationShortcut) -> Vec<Action> {
        match command {
            ApplicationShortcut::StartSelection => {
                self.start_selection();
                Vec::new()
            }
            ApplicationShortcut::Insert => self.insert_selection(),
            _ => Vec::new(),
        }
    }

    fn navigate(&mut self, command: NavigationShortcut) {
        let Some(document) = self.selected_document() else {
            return;
        };
        if let Some(delta) = self.half_page_delta(command) {
            self.navigate_visual_rows(delta);
            return;
        }
        let current_row = document.document.cursor;
        let last_row = document.document.diff.len().saturating_sub(1);
        let target_row = match command {
            NavigationShortcut::MoveDown => current_row.saturating_add(1),
            NavigationShortcut::MoveUp => current_row.saturating_sub(1),
            NavigationShortcut::GoToFirst => 0,
            NavigationShortcut::GoToLast => last_row,
            NavigationShortcut::MoveHalfPageDown | NavigationShortcut::MoveHalfPageUp => {
                unreachable!()
            }
            NavigationShortcut::GoToNextLocation
            | NavigationShortcut::GoToPreviousLocation
            | NavigationShortcut::GoToChildRevision
            | NavigationShortcut::GoToParentRevision
            | NavigationShortcut::OpenRevisionSelector => return,
        };
        let target_row = target_row.min(last_row);
        if matches!(
            command,
            NavigationShortcut::GoToFirst | NavigationShortcut::GoToLast
        ) {
            self.jump_cursor(target_row);
        } else {
            self.move_cursor(target_row);
        }
    }

    fn half_page_delta(&self, command: NavigationShortcut) -> Option<isize> {
        let half_page =
            isize::try_from(usize::from(self.viewport_height) / 2).unwrap_or(isize::MAX);
        match command {
            NavigationShortcut::MoveHalfPageDown => Some(half_page),
            NavigationShortcut::MoveHalfPageUp => Some(-half_page),
            _ => None,
        }
    }

    fn navigate_visual_rows(&mut self, delta: isize) {
        let target = self.selected_document().and_then(|document| {
            let guide_layout = self.guide_layout(document);
            layout_diff_viewport(
                document,
                self.viewport_width,
                self.palette,
                self.search_query(),
                self.selection.map(SelectionState::range),
                true,
                guide_layout,
            )
            .source_position_after_visual_delta(document, delta)
        });
        let Some((row, column)) = target else {
            return;
        };
        self.move_cursor(row);
        if let Some(document) = self.selected_document_mut()
            && let Some((_, line)) = document.document.diff.source_position(row)
        {
            document.document.column = display_column_to_byte(&line, column);
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
            document.document.column = column;
        }
        self.center_jump_target();
        self.publish_viewports();
        self.publish_current_location();
        self.publish_search_status();
    }

    fn set_cursor(&mut self, target: usize) {
        if let Some(document) = self.selected_document_mut() {
            document.document.cursor = target.min(document.document.diff.len().saturating_sub(1));
            document.document.clear_source_location();
        }
        if let Some(selection) = &mut self.selection
            && !selection.fixed
        {
            selection.cursor = target;
        }
    }

    fn keep_cursor_visible(&mut self) {
        self.place_scroll(ScrollPlacement::KeepCursorVisible);
    }

    fn center_jump_target(&mut self) {
        self.place_scroll(ScrollPlacement::CenterCursor);
    }

    fn align_guide_jump_to_viewport_top(&mut self) {
        self.place_scroll(ScrollPlacement::AlignGuideTop);
    }

    fn place_scroll(&mut self, placement: ScrollPlacement) {
        let height = usize::from(self.viewport_height);
        let scroll = self.displayed_document().map(|document| {
            let guide_layout = self.guide_layout(document);
            let viewport = layout_diff_viewport(
                document,
                self.viewport_width,
                self.palette,
                self.search_query(),
                self.selection.map(SelectionState::range),
                true,
                guide_layout,
            );
            match placement {
                ScrollPlacement::KeepCursorVisible => {
                    viewport.scroll_with_cursor_visible(document, height)
                }
                ScrollPlacement::CenterCursor => {
                    viewport.scroll_with_cursor_centered(document, height)
                }
                ScrollPlacement::AlignGuideTop => viewport.scroll_with_guide_top_aligned(document),
            }
        });
        let Some(document) = self.displayed_document_mut() else {
            return;
        };
        if let Some(scroll) = scroll {
            document.document.scroll = scroll;
        }
    }

    fn start_selection(&mut self) {
        let Some(cursor) = self
            .selected_document()
            .map(|document| document.document.cursor)
        else {
            return;
        };
        if !self
            .selected_document()
            .is_some_and(|document| document.document.diff.is_selectable(cursor))
        {
            return;
        }
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
    }

    fn insert_selection(&self) -> Vec<Action> {
        let (Some(document), Some(selection)) = (self.selected_document(), self.selection) else {
            return Vec::new();
        };
        let range = selection.range();
        if !range
            .clone()
            .any(|row| document.document.diff.is_selectable(row))
        {
            return Vec::new();
        }
        document
            .document
            .diff
            .excerpt(range)
            .ok()
            .map(|excerpt| Action::Output {
                target: self.output_target,
                text: excerpt.into_string(),
            })
            .into_iter()
            .collect()
    }

    fn search(&mut self, command: SearchShortcut) -> Vec<Action> {
        let mut actions = Vec::new();
        match command {
            SearchShortcut::Begin => {
                let origin = self.search_cursor_location();
                self.selection = None;
                self.search = Some(SearchState {
                    query: String::new(),
                    origin,
                    editing: true,
                    waiting_for_repository: false,
                    matches: Vec::new(),
                });
                self.refresh_search_matches();
                if let Some(action) = self.load_all_diffs_for_search() {
                    self.search
                        .as_mut()
                        .expect("search was just created")
                        .waiting_for_repository = true;
                    actions.push(action);
                }
            }
            SearchShortcut::WordUnderCursor => {
                if let Some(word) = self.word_under_cursor() {
                    let origin = self.search_cursor_location();
                    self.selection = None;
                    self.search = Some(SearchState {
                        query: word,
                        origin,
                        editing: false,
                        waiting_for_repository: false,
                        matches: Vec::new(),
                    });
                    self.refresh_search_matches();
                    self.repeat_search(SearchDirection::Forward);
                    if let Some(action) = self.load_all_diffs_for_search() {
                        self.search
                            .as_mut()
                            .expect("search was just created")
                            .waiting_for_repository = true;
                        actions.push(action);
                    }
                }
            }
            SearchShortcut::NextMatch => self.repeat_search(SearchDirection::Forward),
            SearchShortcut::PreviousMatch => self.repeat_search(SearchDirection::Backward),
        }
        self.publish_decorations();
        self.publish_search_status();
        actions
    }

    fn edit_search(&mut self, key: Key) {
        match key {
            Key::Char(character) => {
                if let Some(search) = &mut self.search {
                    search.query.push(character);
                }
            }
            Key::Backspace => {
                if let Some(search) = &mut self.search {
                    search.query.pop();
                }
            }
            Key::Enter => {
                if let Some(search) = &mut self.search {
                    search.editing = false;
                }
                self.publish_search_status();
                return;
            }
            Key::Escape => {
                let origin = self.search.take().map(|search| search.origin);
                if let Some(origin) = origin {
                    self.jump_to_search_location(&origin);
                }
                self.publish_decorations();
                self.publish_search_status();
                return;
            }
            _ => return,
        }
        self.refresh_search_matches();
        self.find_from_origin();
        self.publish_decorations();
        self.publish_search_status();
    }

    fn find_from_origin(&mut self) {
        let Some(search) = self.search.clone() else {
            return;
        };
        if search.query.is_empty() {
            self.jump_to_search_location(&search.origin);
            return;
        }
        if let Some(target) = search.first_after(&search.origin).cloned() {
            self.jump_to_search_location(&target);
        }
    }

    fn repeat_search(&mut self, direction: SearchDirection) {
        let origin = self.current_review_location();
        let Some(search) = self.search.as_ref().filter(|search| !search.editing) else {
            return;
        };
        let Some(current) = self.current_search_location() else {
            return;
        };
        let target = match direction {
            SearchDirection::Forward => search.first_after(&current),
            SearchDirection::Backward => search.first_before(&current),
        };
        if let Some(target) = target {
            let target = target.clone();
            self.jump_to_search_location(&target);
            self.record_current_location_jump(origin);
        }
    }

    fn load_all_diffs_for_search(&mut self) -> Option<Action> {
        let review_checkpoint = self.review_checkpoint.clone()?;
        let paths = self
            .documents
            .iter_mut()
            .filter_map(LoadedDocument::start_diff_load)
            .collect::<Vec<_>>();
        (!paths.is_empty()).then_some(Action::LoadDiffs {
            review_checkpoint,
            paths,
        })
    }

    fn refresh_search_matches(&mut self) {
        if let Some(search) = &mut self.search {
            search.replace_matches(&self.documents);
        }
    }

    fn jump_to_search_location(&mut self, location: &RepositorySearchLocation) {
        self.selected_path = Some(location.path.clone());
        self.jump_cursor_to_column(location.position.row, Some(location.position.column));
        self.events.publish(FileSelectionRequested {
            path: location.path.clone(),
        });
    }

    fn word_under_cursor(&self) -> Option<String> {
        let document = self.selected_document()?;
        let (_, line) = document
            .document
            .diff
            .source_position(document.document.cursor)?;
        let mut column = document.document.column.min(line.len());
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
        let Some(document) = self.selected_document_mut() else {
            return Vec::new();
        };
        if command == SourceShortcut::ExpandOrMoveRight
            && document.document.diff.expand(document.document.cursor)
        {
            self.keep_cursor_visible();
            self.publish_viewports();
            self.publish_current_location();
            return Vec::new();
        }
        let source_line = document.document.diff.source_text(document.document.cursor);
        let Some(source_line) = source_line.as_deref() else {
            return Vec::new();
        };
        match command {
            SourceShortcut::MoveLeft => {
                document.document.column =
                    previous_character_column(source_line, document.document.column);
            }
            SourceShortcut::ExpandOrMoveRight => {
                document.document.column =
                    next_character_column(source_line, document.document.column);
            }
            SourceShortcut::MoveToStartOfLine => document.document.column = 0,
            SourceShortcut::MoveToEndOfLine => {
                document.document.column = source_line.len();
            }
            SourceShortcut::MoveToNextWord => {
                document.document.column = next_word_column(source_line, document.document.column);
            }
            SourceShortcut::MoveToPreviousWord => {
                document.document.column =
                    previous_word_column(source_line, document.document.column);
            }
        }
        self.keep_cursor_visible();
        self.publish_viewports();
        self.publish_current_location();
        Vec::new()
    }

    fn lsp(&self, command: LspShortcut) -> Vec<Action> {
        let operation = match command {
            LspShortcut::ShowDocumentation => review_lsp::Operation::Hover,
            LspShortcut::GoToDefinition => review_lsp::Operation::Definition,
            LspShortcut::GoToReferences => review_lsp::Operation::References,
            LspShortcut::Restart => return vec![Action::RestartLsp],
        };
        let (Some(document), Some(checkpoint)) =
            (self.selected_document(), self.review_checkpoint.as_ref())
        else {
            return Vec::new();
        };
        let Some((line, expected_line)) = document
            .document
            .diff
            .source_position(document.document.cursor)
        else {
            return Vec::new();
        };
        let mut byte_column = document.document.column.min(expected_line.len());
        while !expected_line.is_char_boundary(byte_column) {
            byte_column = byte_column.saturating_sub(1);
        }
        let path = document.disk_path.clone().unwrap_or_else(|| {
            let path = PathBuf::from(&document.path);
            if path.is_absolute() {
                path
            } else {
                self.repository_root.join(path)
            }
        });
        vec![Action::Lsp {
            operation,
            query: review_lsp::Query {
                toast_id: toasts::ToastId::generate(),
                path,
                line,
                byte_column,
                expected_line,
                snapshot_id: checkpoint.checkpoint.clone(),
            },
        }]
    }

    fn renderer<'a>(
        &'a self,
        palette: Palette,
        guide_layout: Option<GuideLayout<'a>>,
        focused: bool,
    ) -> DiffRenderer<'a> {
        DiffRenderer::new(
            palette,
            self.displayed_document(),
            guide_layout,
            focused,
            self.selected_path
                .as_deref()
                .is_some_and(|path| self.reviewable_files.contains(path)),
            self.search_query(),
            self.selection.map(SelectionState::range),
        )
    }

    fn repository_changed(&mut self, event: &RepositoryFilesChanged) -> Vec<Action> {
        self.preview = None;
        self.pending_preview_location = None;
        let same_review_unit = self.review_checkpoint.as_ref().is_some_and(|checkpoint| {
            checkpoint.review_unit == event.review_checkpoint.review_unit
        });
        let same_checkpoint = self
            .review_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint == &event.review_checkpoint);
        if !same_review_unit {
            self.pending_center_path = None;
            self.guide_items.clear();
            self.guide_counters.clear();
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
                let mut document = LoadedDocument::new(&path);
                summary
                    .display_path()
                    .clone_into(&mut document.display_path);
                document.disk_path.clone_from(&summary.disk_path);
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
        self.review_checkpoint = Some(event.review_checkpoint.clone());
        self.pending_load_path = None;
        self.refresh_search_matches();
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
            return self.restore_location(&pending.target);
        }
        if self.search.is_some() {
            let action = self.load_all_diffs_for_search();
            if action.is_some() {
                self.search
                    .as_mut()
                    .expect("active search is still present")
                    .waiting_for_repository = true;
            }
            action.into_iter().collect()
        } else {
            self.selected_load_action().into_iter().collect()
        }
    }

    fn file_selected(&mut self, event: &FileSelected) -> Vec<Action> {
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
        self.publish_viewports();
        self.publish_current_location();
        self.publish_search_status();
        self.record_current_location_jump(origin);
        if load_immediately {
            self.selected_load_action().into_iter().collect()
        } else {
            self.pending_load_path = Some(event.path.clone());
            Vec::new()
        }
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
        let document = self
            .documents
            .iter_mut()
            .find(|document| document.path == selected_path)?;
        document.start_diff_load().map(|path| Action::LoadDiff {
            review_checkpoint,
            path,
        })
    }

    fn content_loaded(&mut self, event: &DiffContentLoaded) {
        let syntax_highlighter = self.highlighter.clone();
        let Some(document) = self.current_document_mut(&event.review_checkpoint, &event.path)
        else {
            return;
        };
        let highlighted_rows = syntax_highlighter.highlight(
            &event.path,
            event.rows.clone(),
            event.old_content.as_deref(),
            event.new_content.as_deref(),
        );
        document.replace_diff(DiffPresentation::new(highlighted_rows));
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
        self.refresh_search_matches();
        if self
            .search
            .as_ref()
            .is_some_and(|search| search.editing || search.waiting_for_repository)
        {
            self.find_from_origin();
        }
        self.finish_repository_search_load_if_complete();
        self.publish_viewports();
        self.publish_decorations();
        self.publish_current_location();
        self.publish_search_status();
    }

    fn content_load_failed(&mut self, event: &DiffContentLoadFailed) {
        if self.pending_center_path.as_deref() == Some(&event.path) {
            self.pending_center_path = None;
        }
        if let Some(document) = self.current_document_mut(&event.review_checkpoint, &event.path) {
            document.fail_diff_load();
        }
        self.finish_repository_search_load_if_complete();
    }

    fn finish_repository_search_load_if_complete(&mut self) {
        let repository_is_loading = self
            .documents
            .iter()
            .any(|document| document.document.loading);
        if !repository_is_loading && let Some(search) = &mut self.search {
            search.waiting_for_repository = false;
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

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn output_target_changed(&mut self, event: &OutputTargetChanged) {
        self.output_target = event.output_target;
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn output_finished(&mut self, event: &OutputDeliveryFinished) {
        if event.delivered {
            self.selection = None;
        }
    }

    fn guide_layout_changed(&mut self, event: &GuideLayoutChanged) {
        self.guide_items.clone_from(&event.items);
        self.guide_counters.clone_from(&event.counters);
    }

    fn guide_jump_requested(&mut self, event: &GuideJumpRequested) -> Vec<Action> {
        let origin = self.current_review_location();
        let Some(document) = self.documents.get(event.file_index) else {
            return Vec::new();
        };
        let path = document.path.clone();
        self.selected_path = Some(path.clone());
        if let Some(document) = self.selected_document_mut() {
            document.document.column = 0;
        }
        if let Some(row) = event.row {
            self.set_cursor(row);
        }
        self.align_guide_jump_to_viewport_top();
        self.events.publish(FileSelectionRequested { path });
        self.publish_viewports();
        self.publish_current_location();
        self.record_current_location_jump(origin);
        self.selected_load_action().into_iter().collect()
    }

    fn preview_source_location(&mut self, event: &SourceLocationPreviewRequested) -> Vec<Action> {
        self.preview = None;
        self.pending_preview_location = Some(event.location.clone());
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
                .map(|path| Action::LoadDiff {
                    review_checkpoint,
                    path,
                })
                .into_iter()
                .collect();
        }
        vec![Action::LoadSource {
            snapshot_id: review_checkpoint.checkpoint,
            location: event.location.clone(),
            mode: ui_actions::SourceLoadMode::Preview,
        }]
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
        let Some(review_checkpoint) = &self.review_checkpoint else {
            return Vec::new();
        };
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
                self.events
                    .publish(FileSelectionRequested { path: path.clone() });
                self.publish_viewports();
                self.publish_current_location();
            }
            let action = self.selected_load_action();
            if mode.is_external() && !revealed {
                self.pending_center_path = Some(path);
            }
            return action.into_iter().collect();
        }
        vec![Action::LoadSource {
            snapshot_id: review_checkpoint.checkpoint.clone(),
            location,
            mode,
        }]
    }

    fn source_content_loaded(&mut self, event: &SourceContentLoaded) {
        if self
            .review_checkpoint
            .as_ref()
            .is_none_or(|checkpoint| checkpoint.checkpoint != event.snapshot_id)
        {
            return;
        }
        let review_path = event.location.review_path(&self.repository_root);
        let display_path = event.location.display_path(&self.repository_root);
        let highlighted =
            self.highlighter
                .highlight(&display_path, Vec::new(), None, Some(&event.content));
        let mut presentation = DiffPresentation::new(highlighted);
        let _ = presentation.show_file();
        let mut document = LoadedDocument::from_source(
            &event.location,
            review_path.as_deref(),
            presentation,
            event.mode,
        );
        let _ = document.document.reveal_location(&event.location);
        if event.mode.is_external() {
            self.documents.retain(|document| !document.temporary);
            let path = document.path.clone();
            self.documents.push(document);
            self.selected_path = Some(path.clone());
            self.center_jump_target();
            self.events.publish(TemporaryFilesChanged {
                files: vec![FileSummary::temporary(
                    path.clone(),
                    event.location.path.display().to_string(),
                    event.location.path.clone(),
                )],
            });
            self.events.publish(FileSelectionRequested { path });
            self.publish_viewports();
            self.publish_current_location();
        } else if self.pending_preview_location.as_ref() == Some(&event.location) {
            self.preview = Some(document);
            self.center_jump_target();
        }
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

    fn source_content_failed(&mut self, event: &SourceContentLoadFailed) {
        if self
            .review_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.checkpoint == event.snapshot_id)
        {
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
                self.events
                    .publish(FileSelectionRequested { path: path.clone() });
                self.publish_viewports();
                self.publish_current_location();
                let load_already_active = self
                    .selected_document()
                    .is_some_and(|document| document.document.loading);
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
        let Some(target) = self
            .location_history
            .navigate(direction, current, |location| {
                if location.review_unit() != &current_review_unit {
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
            return vec![Action::EditRevision { change_id }];
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
            cursor: document.document.cursor,
            presentation_location: document
                .document
                .diff
                .presentation_location(document.document.cursor),
            column: document.document.column,
        })
    }

    fn publish_current_location(&self) {
        self.events.publish(CurrentReviewLocationChanged {
            location: self.current_review_location(),
        });
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn reviewable_files_changed(&mut self, _event: &ReviewableFilesChanged) {
        self.publish_viewports();
    }

    fn publish_viewports(&self) {
        let viewports = self
            .documents
            .iter()
            .enumerate()
            .filter(|(_, document)| self.reviewable_files.contains(&document.path))
            .map(|(file_index, document)| document.guide_viewport(file_index))
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
        let notice_paths = self
            .documents
            .iter()
            .filter(|document| document.has_notice())
            .map(|document| document.path.clone())
            .collect();
        let search_match_paths = self.search.as_ref().map_or_else(Vec::new, |search| {
            let mut paths = search
                .matches
                .iter()
                .map(|location| location.path.clone())
                .collect::<Vec<_>>();
            paths.dedup();
            paths
        });
        self.events.publish(FileDecorationsChanged {
            notice_paths,
            search_match_paths,
        });
    }

    fn publish_search_status(&self) {
        let Some(search) = &self.search else {
            self.events.publish(SearchStatusChanged::default());
            return;
        };
        let current_location = self.current_search_location();
        let current_match = current_location
            .and_then(|current_location| {
                search
                    .matches
                    .iter()
                    .position(|location| *location == current_location)
            })
            .map_or(0, |index| index.saturating_add(1));
        self.events.publish(SearchStatusChanged {
            query: Some(search.query.clone()),
            current_match,
            total_matches: search.matches.len(),
        });
    }

    fn search_query(&self) -> Option<&str> {
        self.search.as_ref().map(|search| search.query.as_str())
    }

    fn search_cursor_location(&self) -> RepositorySearchLocation {
        self.current_search_location()
            .unwrap_or_else(|| RepositorySearchLocation {
                document_index: 0,
                path: self.selected_path.clone().unwrap_or_default(),
                position: SearchMatch { row: 0, column: 0 },
            })
    }

    fn current_search_location(&self) -> Option<RepositorySearchLocation> {
        let (document_index, document) =
            self.documents.iter().enumerate().find(|(_, document)| {
                Some(document.path.as_str()) == self.selected_path.as_deref()
            })?;
        Some(RepositorySearchLocation {
            document_index,
            path: document.path.clone(),
            position: SearchMatch {
                row: document.document.cursor,
                column: document.document.column,
            },
        })
    }
}

impl Component<Action> for DiffComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::repository_changed);
        subscriptions.subscribe(Self::file_selected);
        subscriptions.subscribe(Self::animation_tick);
        subscriptions.subscribe(Self::content_loaded);
        subscriptions.subscribe(Self::content_load_failed);
        subscriptions.subscribe(Self::reviewable_files_changed);
        subscriptions.subscribe(Self::viewport_changed);
        subscriptions.subscribe(Self::clear_input);
        subscriptions.subscribe(Self::output_target_changed);
        subscriptions.subscribe(Self::output_finished);
        subscriptions.subscribe(Self::guide_layout_changed);
        subscriptions.subscribe(Self::guide_jump_requested);
        subscriptions.subscribe(Self::preview_source_location);
        subscriptions.subscribe(Self::location_list_visibility_changed);
        subscriptions.subscribe(Self::accept_source_location);
        subscriptions.subscribe(Self::source_content_loaded);
        subscriptions.subscribe(Self::source_content_failed);
        subscriptions.subscribe(Self::restore_review_location);
        subscriptions.subscribe(Self::location_jumped);
        subscriptions.subscribe(Self::revision_edit_failed);
        subscriptions.subscribe_input(
            InputScope::Focused,
            DiffKeyboardInputMatcher::new(),
            Self::keyboard_input,
        );
        subscriptions.subscribe_input(
            InputScope::Hovered,
            component_core::AnyInput,
            Self::pointer_input,
        );
    }
}

#[derive(Clone, Copy)]
enum DiffKeyboardInput {
    SearchKey(Key),
    Shortcut(ShortcutCommand),
}

struct DiffKeyboardInputMatcher {
    shortcuts: ShortcutMatcher,
}

impl DiffKeyboardInputMatcher {
    const fn new() -> Self {
        Self {
            shortcuts: ShortcutMatcher::new(ShortcutSet::Diff),
        }
    }
}

impl InputMatcher<DiffComponent, Key> for DiffKeyboardInputMatcher {
    type Output = DiffKeyboardInput;

    fn resolve(&mut self, component: &DiffComponent, key: &Key) -> InputResolution<Self::Output> {
        if component
            .search
            .as_ref()
            .is_some_and(|search| search.editing)
        {
            return InputResolution::Matched(DiffKeyboardInput::SearchKey(*key));
        }
        match self.shortcuts.resolve_key(*key) {
            InputResolution::NoMatch => InputResolution::NoMatch,
            InputResolution::AwaitingMoreInput => InputResolution::AwaitingMoreInput,
            InputResolution::Matched(command) => {
                InputResolution::Matched(DiffKeyboardInput::Shortcut(command))
            }
        }
    }
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
