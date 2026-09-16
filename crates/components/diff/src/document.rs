//! Loaded diff documents.

use std::path::PathBuf;

use review_lsp::SourceLocation;
use ui_events::DisplayedDiffViewport;

use crate::presentation::DiffPresentation;
use ui_actions::SourceLoadMode;
use ui_events::PresentationLocation;

/// Loaded diff content and its navigation state for one file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DiffDocument {
    pub(super) diff: DiffPresentation,
    pub(super) cursor: usize,
    pub(super) scroll: usize,
    pub(super) column: usize,
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
    pub(super) fn place_cursor(&mut self, position: crate::DiffPointerPosition) {
        self.cursor = position.row.min(self.diff.len().saturating_sub(1));
        if let Some(column) = position.column {
            self.column = self
                .diff
                .source_position(self.cursor)
                .map_or(0, |(_, line)| crate::display_column_to_byte(&line, column));
        }
        self.clear_source_location();
    }

    fn new() -> Self {
        Self {
            diff: DiffPresentation::default(),
            cursor: 0,
            scroll: 0,
            column: 0,
            source_location: None,
            highlighting: None,
            load_state: DiffLoadState::Idle,
        }
    }

    fn preserve_navigation_from(&mut self, previous: &Self) {
        self.cursor = previous.cursor;
        self.scroll = previous.scroll;
        self.column = previous.column;
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
        let cursor_location = self.diff.presentation_location(self.cursor);
        let scroll_location = self.diff.presentation_location(self.scroll);
        let fallback_cursor = self.cursor;
        let fallback_scroll = self.scroll;
        let column = self.column;
        let source_location = self.source_location.clone();
        self.diff = diff;
        let reload_required = self.load_state.finish_load();
        self.restore_presentation_location(cursor_location, fallback_cursor, column);
        self.scroll = scroll_location
            .and_then(|location| self.diff.reveal_presentation_location(location))
            .unwrap_or_else(|| fallback_scroll.min(self.diff.len().saturating_sub(1)));
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
        Some(ui_actions::Action::Highlight(pending.request.clone()))
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
        self.column = location.byte_column;
        self.source_location = Some(location.clone());
        let Some(row) = self.diff.reveal_line(location.line) else {
            return false;
        };
        self.cursor = row;
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
            .unwrap_or_else(|| fallback_row.min(self.diff.len().saturating_sub(1)));
        self.column = column;
        self.clear_source_location();
        self.cursor = row;
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
            .source_position(self.document.cursor)
            .map_or_else(
                || self.document.source_location.clone(),
                |(line, _)| {
                    let byte_column = self.document.column;
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

    pub(super) fn guide_viewport(&self, file_index: usize) -> DisplayedDiffViewport {
        self.document
            .diff
            .guide_viewport(self.path.clone(), file_index)
    }

    pub(super) fn presented_row(&self) -> usize {
        self.document.cursor
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
