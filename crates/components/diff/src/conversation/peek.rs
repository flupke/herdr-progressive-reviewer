//! A private native source viewer preserves the Files document and reply draft.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget},
};
use review_lsp::SourceLocation;
use ui_actions::Action;
use ui_events::{SourceContentLoadFailed, SourceContentLoaded, SourceLoadMode};
use ui_theme::Palette;

use crate::{DiffComponent, comments::Comments};

pub(crate) struct SourcePeek {
    request: String,
    pub(crate) viewer: Box<DiffComponent>,
    status: Option<String>,
}

impl SourcePeek {
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
            .render(body, buffer, palette, focused, None)
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
    pub(crate) fn close_peek(&mut self) {
        if self.conversation.peek.take().is_some() {
            self.events
                .publish(ui_events::SourceSessionChanged { snapshot_id: None });
            self.publish_search_status();
        }
    }

    pub(crate) fn refresh_peek_checkpoint(&mut self) {
        if let Some(peek) = &mut self.conversation.peek {
            peek.viewer
                .review_checkpoint
                .clone_from(&self.review_checkpoint);
        }
    }

    pub(crate) fn source_view(&self) -> &Self {
        self.conversation
            .peek
            .as_ref()
            .map_or(self, |peek| &peek.viewer)
    }

    pub(crate) fn source_view_mut(&mut self) -> &mut Self {
        match self.conversation.peek {
            Some(ref mut peek) => &mut peek.viewer,
            None => self,
        }
    }

    pub(crate) fn source_snapshot(&self) -> Option<&str> {
        self.source_session.as_deref().or_else(|| {
            self.review_checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.checkpoint.as_str())
        })
    }

    pub(super) fn peek_conversation(&mut self) -> Vec<Action> {
        let Some(thread) = self.conversation_thread() else {
            return Vec::new();
        };
        let path = self
            .documents
            .iter()
            .find(|file| !file.comments_only && Comments::matches_path(file, thread.path()))
            .map_or_else(
                || thread.path().to_owned(),
                |file| file.new_path.clone().unwrap_or_else(|| file.path.clone()),
            );
        let line = self
            .comments
            .mapped(&thread.id)
            .and_then(|hunk| hunk.new.as_ref())
            .map_or(0, |lines| lines.start);
        self.conversation.next_peek = self.conversation.next_peek.wrapping_add(1);
        let request = format!("thread-peek:{}", self.conversation.next_peek);
        let mut viewer = Self::new(
            self.events.clone(),
            ui_events::ReviewableFiles::default(),
            self.highlighter.clone(),
            self.repository_root.clone(),
            self.palette,
        );
        viewer.next_search_id = std::rc::Rc::clone(&self.next_search_id);
        viewer.source_session = Some(request.clone());
        viewer.review_checkpoint.clone_from(&self.review_checkpoint);
        viewer.viewport_width = self.viewport_width;
        viewer.viewport_height = self.viewport_height.saturating_sub(1).max(1);
        self.events.publish(ui_events::SourceSessionChanged {
            snapshot_id: Some(request.clone()),
        });
        self.conversation.peek = Some(SourcePeek {
            request: request.clone(),
            viewer: Box::new(viewer),
            status: Some("Loading current file…".into()),
        });
        vec![Action::LoadSource {
            snapshot_id: request,
            location: SourceLocation {
                path: self.repository_root.join(path),
                line,
                byte_column: 0,
                end_line: line,
                end_byte_column: 0,
            },
            mode: SourceLoadMode::ThreadPeek,
        }]
    }

    pub(crate) fn peek_loaded(&mut self, event: &SourceContentLoaded) -> Vec<Action> {
        let highlight = self
            .conversation_thread()
            .and_then(|thread| thread.anchor.map_new_lines(&event.content));
        let Some(peek) = &mut self.conversation.peek else {
            return Vec::new();
        };
        if peek.request != event.snapshot_id {
            return Vec::new();
        }
        let mut source = event.clone();
        source.mode = SourceLoadMode::External;
        // Only reveal a range when the saved anchor still maps to disk content.
        source.location.line = highlight.as_ref().map_or(0, |range| range.start);
        source.location.end_line = highlight
            .as_ref()
            .map_or(0, |range| range.end.saturating_sub(1));
        let actions = peek.viewer.source_content_loaded(&source);
        if let Some(range) = highlight {
            let start = usize::try_from(range.start).unwrap_or_default();
            let end = usize::try_from(range.end.saturating_sub(1)).unwrap_or(start);
            peek.viewer.selection = Some(crate::SelectionState {
                anchor: start,
                cursor: end,
                fixed: true,
            });
        }
        peek.status = None;
        if let Some(id) = &self.conversation.selected {
            self.conversation.unavailable.remove(id);
        }
        self.publish_thread_contexts();
        actions
    }

    pub(crate) fn peek_failed(&mut self, event: &SourceContentLoadFailed) -> bool {
        if let Some(peek) = &mut self.conversation.peek
            && peek.request == event.snapshot_id
            && peek.status.is_some()
        {
            peek.status = Some(format!("Current file unavailable: {}", event.message));
            if let Some(id) = &self.conversation.selected {
                self.conversation.unavailable.insert(id.clone());
            }
            self.publish_thread_contexts();
            return true;
        }
        false
    }
}
