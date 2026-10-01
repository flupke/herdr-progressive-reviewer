//! Loaded diff documents.

use std::path::PathBuf;

use review_lsp::SourceLocation;
use ui_events::DisplayedDiffViewport;

use crate::presentation::DiffPresentation;
use crate::render::{DiffViewport, DocumentRows};
use diff_position::Position;
use ui_actions::SourceLoadMode;
use ui_events::PresentationLocation;

/// Loaded diff content and its navigation state for one file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DiffDocument {
    pub(super) diff: DiffPresentation,
    position: Position,
    pub(super) source_location: Option<SourceLocation>,
    highlighting: Option<PendingHighlight>,
    load_state: DiffLoadState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingHighlight {
    request: ui_events::HighlightRequest,
    submitted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffLoadState {
    Idle,
    ReloadRequired,
    Loading,
    ReloadLoading,
    LoadingThenReload,
}

impl DiffLoadState {
    fn finish_load(&mut self) -> bool {
        let reload_required = *self == Self::LoadingThenReload;
        *self = if reload_required {
            Self::ReloadRequired
        } else {
            Self::Idle
        };
        reload_required
    }

    fn fail_load(&mut self) -> bool {
        let reload_immediately = *self == Self::LoadingThenReload;
        *self = match self {
            Self::ReloadLoading | Self::LoadingThenReload => Self::ReloadRequired,
            _ => Self::Idle,
        };
        reload_immediately
    }

    fn require_reload(&mut self) {
        *self = match self {
            Self::Loading | Self::ReloadLoading | Self::LoadingThenReload => {
                Self::LoadingThenReload
            }
            _ => Self::ReloadRequired,
        };
    }

    fn start_load(&mut self) -> bool {
        *self = match self {
            Self::Idle => Self::Loading,
            Self::ReloadRequired => Self::ReloadLoading,
            Self::Loading | Self::ReloadLoading | Self::LoadingThenReload => return false,
        };
        true
    }

    fn is_loading(self) -> bool {
        matches!(
            self,
            Self::Loading | Self::ReloadLoading | Self::LoadingThenReload
        )
    }
}

impl DiffDocument {
    /// Where the reader is in this document.
    pub(super) fn position(&self) -> &Position {
        &self.position
    }

    /// The position with this document laid out on `viewport`, for the
    /// position's screen operations.
    pub(super) fn on_screen<'a>(
        &'a mut self,
        viewport: &'a DiffViewport,
    ) -> (&'a mut Position, DocumentRows<'a>) {
        (&mut self.position, DocumentRows::new(viewport, &self.diff))
    }

    /// This document laid out on `viewport`.
    pub(super) fn laid_out<'a>(&'a self, viewport: &'a DiffViewport) -> DocumentRows<'a> {
        DocumentRows::new(viewport, &self.diff)
    }

    /// Move the cursor to `row`, keeping it inside the document.
    pub(super) fn move_cursor(&mut self, row: usize) {
        self.position.move_to(row, self.diff.len());
    }

    /// Move the cursor to a byte column of its line.
    pub(super) fn set_column(&mut self, column: usize) {
        self.position.set_column(column);
    }

    /// Take a saved position, fitted to this document.
    pub(super) fn restore(&mut self, saved: Position) {
        self.position.restore(saved, self.diff.len());
    }

    /// Take a saved position, keeping a full screen of `height` rows.
    pub(super) fn restore_filling_screen(&mut self, saved: Position, height: usize) {
        self.position
            .restore_filling_screen(saved, self.diff.len(), height);
    }

    /// Reveal visual evidence `rows` out of `total` on a screen of `height`.
    pub(super) fn reveal_evidence(
        &mut self,
        rows: std::ops::Range<usize>,
        total: usize,
        height: usize,
    ) {
        self.position.reveal_evidence(rows, total, height);
    }

    pub(super) fn place_cursor(&mut self, position: crate::DiffPointerPosition) {
        self.move_cursor(position.row);
        if let Some(column) = position.column {
            let column = self
                .diff
                .source_position(self.position.cursor())
                .map_or(0, |(_, line)| crate::display_column_to_byte(&line, column));
            self.position.set_column(column);
        }
        self.clear_source_location();
    }

    fn new() -> Self {
        Self {
            diff: DiffPresentation::default(),
            position: Position::default(),
            source_location: None,
            highlighting: None,
            load_state: DiffLoadState::Idle,
        }
    }

    fn preserve_navigation_from(&mut self, previous: &Self) {
        self.position = previous.position;
    }

    fn preserve_loaded_content_from(&mut self, previous: &Self, content_is_current: bool) {
        self.highlighting.clone_from(&previous.highlighting);
        self.source_location.clone_from(&previous.source_location);
        self.diff.clone_from(&previous.diff);
        self.load_state = if content_is_current {
            previous.load_state
        } else {
            DiffLoadState::ReloadRequired
        };
    }

    fn replace_diff(&mut self, diff: DiffPresentation) -> bool {
        self.highlighting = None;
        let previous = self.position;
        let cursor_location = self.diff.presentation_location(previous.cursor());
        let scroll_location = self.diff.presentation_location(previous.scroll());
        let source_location = self.source_location.clone();
        self.diff = diff;
        let reload_required = self.load_state.finish_load();
        // A line the reload folded into a reviewed hunk stays folded.
        let mut restore = |location| {
            self.diff
                .fold_at(location)
                .or_else(|| self.diff.reveal_presentation_location(location))
        };
        let cursor = cursor_location
            .and_then(&mut restore)
            .unwrap_or(previous.cursor());
        let scroll = scroll_location
            .and_then(&mut restore)
            .unwrap_or(previous.scroll());
        self.restore(Position::new(cursor, previous.column(), scroll));
        self.clear_source_location();
        if let Some(source_location) = source_location {
            let _ = self.reveal_location(&source_location);
        }
        reload_required
    }

    pub(super) fn prepare_highlighting(&mut self, request: ui_events::HighlightRequest) {
        self.highlighting = Some(PendingHighlight {
            request,
            submitted: false,
        });
    }

    pub(super) fn request_highlighting(&mut self) -> Option<ui_actions::Action> {
        let pending = self
            .highlighting
            .as_mut()
            .filter(|pending| !pending.submitted)?;
        pending.submitted = true;
        Some(ui_actions::Action::Document(
            ui_actions::DocumentAction::Highlight(pending.request.clone()),
        ))
    }

    pub(super) fn finish_highlighting(&mut self, result: &ui_events::HighlightingFinished) {
        if self
            .highlighting
            .as_ref()
            .is_some_and(|pending| pending.request.same_content(&result.request))
        {
            self.diff.apply_highlights(&result.highlighted);
            self.highlighting = None;
        }
    }

    pub(super) fn reveal_location(&mut self, location: &SourceLocation) -> bool {
        self.position.set_column(location.byte_column);
        self.source_location = Some(location.clone());
        let Some(row) = self.diff.reveal_line(location.line) else {
            return false;
        };
        self.move_cursor(row);
        true
    }

    pub(super) fn restore_presentation_location(
        &mut self,
        location: Option<PresentationLocation>,
        fallback_row: usize,
        column: usize,
    ) {
        let row = location
            .and_then(|location| self.diff.reveal_presentation_location(location))
            .unwrap_or(fallback_row);
        self.position.set_column(column);
        self.clear_source_location();
        self.move_cursor(row);
    }

    pub(super) fn clear_source_location(&mut self) {
        self.source_location = None;
    }
}

impl Default for DiffDocument {
    fn default() -> Self {
        Self::new()
    }
}

/// One loaded diff document and its stable path identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LoadedDocument {
    pub(super) path: String,
    pub(super) display_path: String,
    pub(super) disk_path: Option<PathBuf>,
    pub(super) temporary: bool,
    pub(super) document: DiffDocument,
    pub(super) content: Option<std::sync::Arc<ui_events::DiffContentLoaded>>,
    pub(super) old_path: Option<String>,
    pub(super) new_path: Option<String>,
    pub(super) comments_only: bool,
}

impl diff_search::SearchedDocument for LoadedDocument {
    fn path(&self) -> &str {
        &self.path
    }

    fn text(&self) -> std::sync::Arc<text_search::Document> {
        self.document.diff.search_document()
    }
}

impl LoadedDocument {
    pub(super) fn from_summary(summary: &ui_events::FileSummary) -> Self {
        let mut document = Self::new(summary.path());
        document.old_path = summary
            .file
            .old_path
            .as_ref()
            .map(review_repository::repository::RepoPath::display);
        document.new_path = summary
            .file
            .new_path
            .as_ref()
            .map(review_repository::repository::RepoPath::display);
        summary
            .display_path()
            .clone_into(&mut document.display_path);
        document.disk_path.clone_from(&summary.disk_path);
        document
    }

    /// Create an unloaded document for one repository path.
    pub(super) fn new(path: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            display_path: path.clone(),
            path,
            disk_path: None,
            temporary: false,
            document: DiffDocument::new(),
            content: None,
            old_path: None,
            new_path: None,
            comments_only: false,
        }
    }

    pub(super) fn from_source(
        location: &SourceLocation,
        review_path: Option<&str>,
        diff: DiffPresentation,
        mode: SourceLoadMode,
    ) -> Self {
        let external = mode.is_external();
        let tree_path = if external {
            location.path.file_name().map_or_else(
                || location.path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
        } else {
            review_path.map_or_else(|| location.path.display().to_string(), str::to_owned)
        };
        let mut file = Self::new(tree_path);
        file.display_path = if external {
            location.path.display().to_string()
        } else {
            review_path.map_or_else(|| location.path.display().to_string(), str::to_owned)
        };
        file.document.diff = diff;
        file.temporary = true;
        file.disk_path = Some(location.path.clone());
        file
    }

    pub(super) fn cursor_location(&self) -> Option<SourceLocation> {
        self.document
            .diff
            .source_position(self.document.position.cursor())
            .map_or_else(
                || self.document.source_location.clone(),
                |(line, _)| {
                    let byte_column = self.document.position.column();
                    Some(SourceLocation {
                        path: self
                            .disk_path
                            .clone()
                            .unwrap_or_else(|| PathBuf::from(&self.path)),
                        line,
                        byte_column,
                        end_line: line,
                        end_byte_column: byte_column,
                    })
                },
            )
    }

    pub(super) fn preserve_state_from(
        &mut self,
        previous: &Self,
        preserve_content: bool,
        content_is_current: bool,
    ) {
        self.document.preserve_navigation_from(&previous.document);
        if preserve_content {
            self.content.clone_from(&previous.content);
            self.document
                .preserve_loaded_content_from(&previous.document, content_is_current);
        }
    }

    pub(super) fn replace_diff(&mut self, diff: DiffPresentation) -> bool {
        self.document.replace_diff(diff)
    }

    pub(super) fn fail_diff_load(&mut self) -> bool {
        self.document.load_state.fail_load()
    }

    /// Keep the displayed content while requiring the next load to replace it.
    pub(super) fn require_diff_reload(&mut self) {
        self.document.load_state.require_reload();
    }

    pub(super) fn target_viewport(&self, file_index: usize) -> DisplayedDiffViewport {
        self.document
            .diff
            .target_viewport(self.path.clone(), file_index)
    }

    pub(super) fn presented_row(&self) -> usize {
        self.document.position.cursor()
    }

    pub(super) fn has_notice(&self) -> bool {
        self.document.diff.has_notice()
    }

    pub(super) fn start_diff_load(&mut self) -> Option<String> {
        if self.comments_only || self.document.load_state.is_loading() {
            return None;
        }
        if self.document.load_state != DiffLoadState::ReloadRequired
            && (!self.document.diff.is_empty() || self.document.diff.can_show_file())
        {
            return None;
        }
        let load_started = self.document.load_state.start_load();
        debug_assert!(load_started);
        Some(self.path.clone())
    }

    pub(super) fn is_loading(&self) -> bool {
        self.document.load_state.is_loading()
    }
}
