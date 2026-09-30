//! A private native source viewer preserves the Files document and reply draft.

use diff_position::Position;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget},
};
use review_lsp::SourceLocation;
use ui_actions::{Action, DocumentAction, DocumentLoad};
use ui_events::{SourceContentLoadFailed, SourceContentLoaded, SourceLoadMode};
use ui_theme::Palette;

use crate::{DiffComponent, Role, SourceViewer};

pub(crate) struct SourcePeek {
    request: String,
    location: SourceLocation,
    position: Option<Position>,
    viewer: SourceViewer,
    status: Option<String>,
}

impl SourcePeek {
    pub(crate) fn viewer(&self) -> &SourceViewer {
        &self.viewer
    }

    pub(crate) fn viewer_mut(&mut self) -> &mut SourceViewer {
        &mut self.viewer
    }

    /// The live source whose disk changes must refresh the peek.
    pub(crate) fn source_path(&self) -> &std::path::Path {
        self.viewer
            .selected_document()
            .and_then(|file| file.disk_path.as_deref())
            .unwrap_or(&self.location.path)
    }

    pub(crate) fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette, focused: bool) {
        Paragraph::new(Line::raw(
            " ← Back to thread [Esc] · Current file · read only",
        ))
        .render(
            Rect::new(area.x, area.y, area.width, area.height.min(1)),
            buffer,
        );
        let body = Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width,
            area.height.saturating_sub(1),
        );
        self.viewer
            .render(body, buffer, palette, focused)
            .render(buffer);
        if let Some(status) = &self.status {
            Paragraph::new(status.as_str()).render(
                Rect::new(
                    body.x.saturating_add(1),
                    body.y.saturating_add(1),
                    body.width.saturating_sub(2),
                    body.height.saturating_sub(2),
                ),
                buffer,
            );
        }
    }
}

impl DiffComponent {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(crate) fn refresh_current_file(
        &mut self,
        _: &ui_events::RepositoryRefreshStarted,
    ) -> Vec<Action> {
        let Some(peek) = self.visible_peek_mut() else {
            return Vec::new();
        };
        if let Some(file) = peek.viewer.selected_document() {
            peek.position = Some(*file.document.position());
            if let Some(path) = &file.disk_path {
                peek.location.path.clone_from(path);
            }
        }
        let request = self.next_peek_request();
        let peek = self.conversation.peek.as_mut().expect("the peek is open");
        peek.request.clone_from(&request);
        peek.viewer.source_session = Some(request.clone());
        peek.status = Some("Refreshing current file…".into());
        self.services
            .events
            .publish(ui_events::SourceSessionChanged {
                snapshot_id: Some(request.clone()),
            });
        vec![Action::Document(DocumentAction::Load(
            DocumentLoad::Source {
                snapshot_id: request,
                location: peek.location.clone(),
                mode: SourceLoadMode::ThreadPeek,
            },
        ))]
    }

    fn next_peek_request(&mut self) -> String {
        self.conversation.next_peek = self.conversation.next_peek.wrapping_add(1);
        format!("thread-peek:{}", self.conversation.next_peek)
    }

    pub(crate) fn close_peek(&mut self) {
        if self.conversation.peek.take().is_some() {
            self.services
                .events
                .publish(ui_events::SourceSessionChanged { snapshot_id: None });
            self.files.publish_search_status();
        }
    }

    /// Give the peek the checkpoint the Files viewer now shows.
    pub(crate) fn refresh_peek_checkpoint(&mut self) {
        if let Some(peek) = &mut self.conversation.peek {
            peek.viewer
                .review_checkpoint
                .clone_from(&self.files.review_checkpoint);
        }
    }

    pub(super) fn peek_conversation(&mut self) -> Vec<Action> {
        let Some(thread) = self.conversation_thread() else {
            return Vec::new();
        };
        let files = &self.files;
        let path = files
            .documents
            .iter()
            .find(|file| !file.comments_only && files.comments.matches_path(file, thread.path()))
            .map_or_else(
                || thread.path().to_owned(),
                |file| file.new_path.clone().unwrap_or_else(|| file.path.clone()),
            );
        let line = files
            .comments
            .mapped(&thread.id)
            .and_then(|hunk| hunk.new.as_ref())
            .map_or(0, |lines| lines.start);
        let request = self.next_peek_request();
        let mut viewer = SourceViewer::new(
            &self.services,
            Role::Peek,
            ui_events::ReviewableFiles::default(),
        );
        viewer.source_session = Some(request.clone());
        viewer
            .review_checkpoint
            .clone_from(&self.files.review_checkpoint);
        viewer.viewport_width = self.files.viewport_width;
        viewer.viewport_height = self.files.viewport_height.saturating_sub(1).max(1);
        self.services
            .events
            .publish(ui_events::SourceSessionChanged {
                snapshot_id: Some(request.clone()),
            });
        let location = SourceLocation {
            path: self.services.repository_root.join(&path),
            line,
            byte_column: 0,
            end_line: line,
            end_byte_column: 0,
        };
        self.conversation.peek = Some(SourcePeek {
            request: request.clone(),
            viewer,
            status: Some("Loading current file…".into()),
            location: location.clone(),
            position: None,
        });
        vec![Action::Document(DocumentAction::Load(
            DocumentLoad::Source {
                snapshot_id: request,
                location,
                mode: SourceLoadMode::ThreadPeek,
            },
        ))]
    }

    pub(crate) fn peek_loaded(&mut self, event: &SourceContentLoaded) -> Vec<Action> {
        let highlight = self
            .conversation_thread()
            .and_then(|thread| thread.anchor.map_new_lines(&event.content));
        let Some(peek) = self.visible_peek_mut() else {
            return Vec::new();
        };
        if peek.request != event.snapshot_id {
            return Vec::new();
        }
        let actions = peek.show(event, highlight);
        if let Some(id) = self.conversation.selected.clone() {
            self.files.comments.set_unavailable(id, false);
        }
        self.files.place_threads();
        actions
    }

    pub(crate) fn peek_failed(&mut self, event: &SourceContentLoadFailed) -> bool {
        let Some(peek) = self.visible_peek_mut() else {
            return false;
        };
        if peek.request != event.snapshot_id || peek.status.is_none() {
            return false;
        }
        peek.status = Some(format!("Current file unavailable: {}", event.message));
        peek.viewer.documents.clear();
        peek.viewer.selection = None;
        if let Some(id) = self.conversation.selected.clone() {
            self.files.comments.set_unavailable(id, true);
        }
        self.files.place_threads();
        true
    }
}

impl SourcePeek {
    /// Show the loaded current file, selecting the lines the thread's anchor
    /// still maps to.
    fn show(
        &mut self,
        event: &SourceContentLoaded,
        highlight: Option<std::ops::Range<u32>>,
    ) -> Vec<Action> {
        let mut source = event.clone();
        if let Some(file) = self.viewer.selected_document() {
            self.position = Some(*file.document.position());
        }
        source.mode = SourceLoadMode::External;
        // Only reveal a range when the saved anchor still maps to disk content.
        source.location.line = highlight.as_ref().map_or(0, |range| range.start);
        source.location.end_line = highlight
            .as_ref()
            .map_or(0, |range| range.end.saturating_sub(1));
        let actions = self.viewer.source_content_loaded(&source);
        if let Some(position) = self.position {
            let height = usize::from(self.viewer.viewport_height);
            if let Some(file) = self.viewer.selected_document_mut() {
                file.document.restore_filling_screen(position, height);
            }
        }
        if let Some(range) = highlight {
            let start = usize::try_from(range.start).unwrap_or_default();
            let end = usize::try_from(range.end.saturating_sub(1)).unwrap_or(start);
            self.viewer.selection = Some(crate::SelectionState {
                anchor: start,
                cursor: end,
                fixed: true,
            });
        }
        self.status = None;
        actions
    }
}
