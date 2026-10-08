use std::cell::Ref;

use diff_rendering::{DiffFrame, FrameOverlay, FrameOverlayRow, FrameRule};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Widget,
};
use review_threads::{ReviewThread, ThreadId};
use ui_controls::NavigationLink;
use ui_frame::Frame;
use ui_theme::Palette;

use super::{ConversationAction, ConversationAnchor};
use crate::{
    DiffComponent,
    comment_layout::CommentRow,
    comments::{CommentTarget, Comments, ConversationButton},
};

struct ConversationRows {
    frame: DiffFrame,
    width: u16,
    rows: Vec<CommentRow>,
}

/// A thread laid out for a width, which a frame and a scroll step reuse until the width, the
/// thread or its original code changes. The palette stays the same for the session.
pub(super) struct LaidOutThread {
    width: u16,
    thread: Option<ReviewThread>,
    /// The thread whose original code the rows show.
    original: Option<ThreadId>,
    rows: Vec<CommentRow>,
}

/// The rows of the shown thread: its code and messages, laid out once, then the reply under them,
/// laid out at each frame as the reviewer edits it.
struct ConversationViewport<'a> {
    thread: Ref<'a, [CommentRow]>,
    reply: Vec<CommentRow>,
    /// Where the editor starts in `reply`: the view keeps it at its bottom.
    editor_start: usize,
    content_height: usize,
    height: usize,
}

impl<'a> ConversationViewport<'a> {
    fn new(thread: Ref<'a, [CommentRow]>, reply: Vec<CommentRow>, height: usize) -> Self {
        let editor_start = reply
            .iter()
            .position(|row| row.editor)
            .unwrap_or(reply.len());
        let content = thread.len().saturating_add(editor_start);
        let editor = reply.len().saturating_sub(editor_start);
        Self {
            content_height: content.min(height.saturating_sub(editor)),
            thread,
            reply,
            editor_start,
            height,
        }
    }

    fn content(&self) -> impl Iterator<Item = &CommentRow> {
        self.thread
            .iter()
            .chain(self.reply.iter().take(self.editor_start))
    }

    fn scroll_limit(&self) -> usize {
        self.thread
            .len()
            .saturating_add(self.editor_start)
            .saturating_sub(self.content_height)
    }

    fn visible(&self, scroll: usize) -> impl Iterator<Item = &CommentRow> {
        let scroll = scroll.min(self.scroll_limit());
        self.content()
            .skip(scroll)
            .take(self.content_height)
            .chain(self.reply.iter().skip(self.editor_start))
            .take(self.height)
    }
}

impl ConversationRows {
    fn new(width: u16, number_width: usize, palette: Palette) -> Self {
        Self {
            frame: DiffFrame::new(width, number_width, Style::default().fg(palette.focus)),
            width,
            rows: Vec::new(),
        }
    }

    fn rule(&mut self, rule: FrameRule<'_>) {
        self.rows.push(CommentRow {
            rendered: self.frame.rule(rule, 0),
            target: None,
            editor: false,
            reply: None,
        });
    }

    fn line(&mut self, line: &Line<'static>, action: Option<&ConversationAction>) {
        self.rows.extend(
            self.frame
                .wrapped_content(line, 0)
                .into_iter()
                .map(|mut rendered| {
                    rendered.border_cells.clear();
                    CommentRow {
                        rendered,
                        target: action.cloned().map(CommentTarget::Conversation),
                        editor: false,
                        reply: None,
                    }
                }),
        );
    }

    fn file_link(&mut self, path: &str, palette: Palette) {
        let link = NavigationLink::new(path);
        let label_line = Line::from(Span::styled(link.text(), NavigationLink::style(palette)));
        for (rendered, columns) in self.frame.wrapped_content_with_ranges(&label_line, 0) {
            let links = vec![ConversationButton {
                action: ConversationAction::OpenFile,
                columns,
            }];
            self.rows.push(CommentRow {
                rendered,
                target: Some(CommentTarget::ConversationButtons(links)),
                editor: false,
                reply: None,
            });
        }
    }

    fn text(&mut self, text: impl Into<String>, style: Style) {
        self.line(&Line::raw(text.into()).style(style), None);
    }

    /// The rows, each in the frame's borders.
    fn enclosed(mut self) -> Vec<CommentRow> {
        for row in &mut self.rows {
            if row.rendered.border_cells.is_empty() {
                row.rendered.border_cells = self.frame.enclose_line(&mut row.rendered.line);
            }
        }
        self.rows
    }
}

impl DiffComponent {
    pub(crate) fn render_conversation(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        palette: Palette,
        focused: bool,
    ) {
        let block = Frame::Pane { focused }.block(palette, "Review thread");
        let body = block.inner(area);
        block.render(area, buffer);
        let viewport = self.conversation_viewport(body.width, body.height, palette);
        let scroll = self
            .conversation_scroll(&viewport.thread)
            .min(viewport.scroll_limit());
        self.files.reply_visibility.borrow_mut().observe(
            viewport
                .content()
                .map(|row| (row.reply.as_ref(), &row.rendered.line)),
            scroll..scroll.saturating_add(viewport.content_height),
            body,
        );
        let mut targets = Vec::new();
        let mut overlay = Vec::new();
        for (row, content) in viewport.visible(scroll).enumerate() {
            targets.push(content.target.clone());
            overlay.push(FrameOverlayRow {
                row: u16::try_from(row).unwrap_or(u16::MAX),
                line: Some(content.rendered.line.clone()),
                border_cells: content.rendered.border_cells.clone(),
            });
        }
        FrameOverlay::new(body, overlay).render(buffer);
        self.conversation.targets.replace(targets);
    }

    fn conversation_viewport(
        &self,
        width: u16,
        height: u16,
        palette: Palette,
    ) -> ConversationViewport<'_> {
        let thread = self.conversation_thread();
        let original = self.conversation.original.as_ref().map(|code| &code.thread);
        let fresh = self
            .conversation
            .laid_out
            .borrow()
            .as_ref()
            .is_some_and(|laid_out| {
                laid_out.width == width
                    && laid_out.thread.as_ref() == thread
                    && laid_out.original.as_ref() == original
            });
        if !fresh {
            let rows = self.lay_out_thread(width, palette);
            self.conversation.laid_out.replace(Some(LaidOutThread {
                width,
                thread: thread.cloned(),
                original: original.cloned(),
                rows,
            }));
        }
        let rows = Ref::map(self.conversation.laid_out.borrow(), |laid_out| {
            laid_out
                .as_ref()
                .map_or(&[][..], |laid_out| laid_out.rows.as_slice())
        });
        let reply = self.conversation_reply(width, palette);
        ConversationViewport::new(rows, reply, usize::from(height))
    }

    fn new_conversation_rows(&self, width: u16, palette: Palette) -> ConversationRows {
        let number_width = self
            .conversation
            .original
            .as_ref()
            .map_or(0, |code| code.number_width);
        ConversationRows::new(width, number_width, palette)
    }

    /// The shown thread's code, its unread notice and its messages.
    fn lay_out_thread(&self, width: u16, palette: Palette) -> Vec<CommentRow> {
        let mut output = self.new_conversation_rows(width, palette);
        let Some(thread) = self.conversation_thread() else {
            output.text(
                "Select a thread to read its conversation.",
                Style::default().fg(palette.dim),
            );
            return output.rows;
        };
        output.rule(FrameRule::Top(None));
        self.context_rows(thread, palette, &mut output);
        if thread.has_unread_replies() {
            output.line(
                &Line::from(vec![
                    Span::styled("●", Style::default().fg(palette.deletion)),
                    Span::styled(
                        " New reply · u mark read",
                        Style::default().fg(palette.focus),
                    ),
                ]),
                Some(&ConversationAction::Read),
            );
        }
        output.rule(FrameRule::Middle);
        output.rows.extend(Comments::conversation_messages(
            thread,
            output.frame,
            palette,
        ));
        output.enclosed()
    }

    fn conversation_reply(&self, width: u16, palette: Palette) -> Vec<CommentRow> {
        let Some(thread) = self.conversation_thread() else {
            return Vec::new();
        };
        let mut output = self.new_conversation_rows(width, palette);
        output.rows = self
            .files
            .comments
            .conversation_reply(thread, output.frame, palette);
        output.rule(FrameRule::Bottom);
        output.enclosed()
    }

    /// Where the view scrolls to: what it keeps in sight as the thread opens, else where the
    /// reviewer scrolled. The view clamps the end of the thread to its bottom.
    fn conversation_scroll(&self, rows: &[CommentRow]) -> usize {
        match &self.conversation.anchor {
            Some(ConversationAnchor::Reply(anchor)) => rows
                .iter()
                .position(
                    |row| matches!(&row.target, Some(CommentTarget::Message(id)) if id == anchor),
                )
                .unwrap_or(self.conversation.scroll),
            Some(ConversationAnchor::End) => usize::MAX,
            None => self.conversation.scroll,
        }
    }

    /// Turns what the view kept in sight into a plain scroll position, as the reviewer starts to
    /// scroll.
    pub(in crate::conversation) fn settle_conversation_scroll(&mut self) {
        if self.conversation.anchor.is_none() {
            return;
        }
        // The view shows the reply clamped to the end: the scroll starts from what it shows.
        let scroll = {
            let viewport = self.pane_conversation_viewport();
            self.conversation_scroll(&viewport.thread)
                .min(viewport.scroll_limit())
        };
        self.conversation.scroll = scroll;
        self.conversation.anchor = None;
    }

    pub(in crate::conversation) fn conversation_scroll_limit(&self) -> usize {
        self.pane_conversation_viewport().scroll_limit()
    }

    fn pane_conversation_viewport(&self) -> ConversationViewport<'_> {
        self.conversation_viewport(
            self.files.viewport_width,
            self.files.viewport_height,
            self.services.palette,
        )
    }

    fn context_rows(&self, thread: &ReviewThread, palette: Palette, output: &mut ConversationRows) {
        let Some(path) = thread.path() else {
            output.text(
                "Round conversation of an Explore round",
                Style::default().fg(palette.dim),
            );
            return;
        };
        output.file_link(path, palette);
        output.rule(FrameRule::Middle);
        self.original_context_rows(palette, output);
    }

    fn original_context_rows(&self, palette: Palette, output: &mut ConversationRows) {
        let Some(code) = &self.conversation.original else {
            return;
        };
        output
            .rows
            .extend(crate::render::DiffRenderer::original_context_rows(
                &code.rows,
                output.width,
                code.number_width,
                palette,
            ));
    }
}
