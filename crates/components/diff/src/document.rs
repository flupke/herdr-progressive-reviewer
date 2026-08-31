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
    pub(super) loading: bool,
    reload_required: bool,
}

impl DiffDocument {
    fn new() -> Self {
        Self {
            diff: DiffPresentation::default(),
            cursor: 0,
            scroll: 0,
            column: 0,
            source_location: None,
            loading: false,
            reload_required: false,
        }
    }

    fn preserve_navigation_from(&mut self, previous: &Self) {
        self.cursor = previous.cursor;
        self.scroll = previous.scroll;
        self.column = previous.column;
    }

    fn preserve_loaded_content_from(&mut self, previous: &Self, content_is_current: bool) {
        self.source_location.clone_from(&previous.source_location);
        self.diff.clone_from(&previous.diff);
        self.loading = content_is_current && previous.loading;
        self.reload_required = if content_is_current {
            previous.reload_required
        } else {
            true
        };
    }

    fn replace_diff(&mut self, diff: DiffPresentation) {
        let cursor_location = self.diff.presentation_location(self.cursor);
        let scroll_location = self.diff.presentation_location(self.scroll);
        let fallback_cursor = self.cursor;
        let fallback_scroll = self.scroll;
        let column = self.column;
        let source_location = self.source_location.clone();
        self.diff = diff;
        self.loading = false;
        self.reload_required = false;
        self.restore_presentation_location(cursor_location, fallback_cursor, column);
        self.scroll = scroll_location
            .and_then(|location| self.diff.reveal_presentation_location(location))
            .unwrap_or_else(|| fallback_scroll.min(self.diff.len().saturating_sub(1)));
        if let Some(source_location) = source_location {
            let _ = self.reveal_location(&source_location);
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
}

impl LoadedDocument {
    /// Create an unloaded document for one repository path.
    pub(super) fn new(path: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            display_path: path.clone(),
            path,
            disk_path: None,
            temporary: false,
            document: DiffDocument::new(),
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
            self.document
                .preserve_loaded_content_from(&previous.document, content_is_current);
        }
    }

    pub(super) fn replace_diff(&mut self, diff: DiffPresentation) {
        self.document.replace_diff(diff);
    }

    pub(super) fn fail_diff_load(&mut self) {
        self.document.loading = false;
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
        if self.document.loading {
            return None;
        }
        if !self.document.reload_required
            && (!self.document.diff.is_empty() || self.document.diff.can_show_file())
        {
            return None;
        }
        self.document.loading = true;
        Some(self.path.clone())
    }
}
