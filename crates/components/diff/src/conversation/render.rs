use guide_rendering::{DiffFrame, FrameRule, GuideOverlay, GuideOverlayRow};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Widget},
};
use review_threads::ReviewThread;
use ui_controls::NavigationLink;
use ui_theme::Palette;

use super::ConversationAction;
use crate::{
    DiffComponent,
    comment_layout::CommentRow,
    comments::{CommentTarget, ConversationButton},
};

struct ConversationRows {
    frame: DiffFrame,
    width: u16,
    rows: Vec<CommentRow>,
}

struct ConversationViewport {
    content: Vec<CommentRow>,
    editor: Vec<CommentRow>,
    content_height: usize,
    height: usize,
}

impl ConversationViewport {
    fn new(mut content: Vec<CommentRow>, height: usize) -> Self {
        let editor_start = content
            .iter()
            .position(|row| row.editor)
            .unwrap_or(content.len());
        let editor = content.split_off(editor_start);
        Self {
            content_height: content.len().min(height.saturating_sub(editor.len())),
            content,
            editor,
            height,
        }
    }

    fn scroll_limit(&self) -> usize {
        self.content.len().saturating_sub(self.content_height)
    }

    fn visible(self, scroll: usize) -> Vec<CommentRow> {
        let scroll = scroll.min(self.scroll_limit());
        self.content
            .into_iter()
            .skip(scroll)
            .take(self.content_height)
            .chain(self.editor)
            .take(self.height)
            .collect()
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
}

impl DiffComponent {
    pub(crate) fn render_conversation(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        palette: Palette,
        focused: bool,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Review thread ")
            .border_style(Style::default().fg(if focused { palette.focus } else { palette.dim }));
        let inner = block.inner(area);
        block.render(area, buffer);
        let rows = self.conversation_rows(inner.width, palette);
        let scroll = self.conversation.scroll;
        let mut targets = Vec::new();
        let body = inner;
        let mut overlay = Vec::new();
        let viewport = ConversationViewport::new(rows, usize::from(body.height));
        let scroll = scroll.min(viewport.scroll_limit());
        self.reply_visibility.borrow_mut().observe(
            viewport
                .content
                .iter()
                .map(|row| (row.reply.as_ref(), &row.rendered.line)),
            scroll..scroll.saturating_add(viewport.content_height),
            body,
        );
        for (row, content) in viewport.visible(scroll).into_iter().enumerate() {
            targets.push(content.target);
            overlay.push(GuideOverlayRow {
                row: u16::try_from(row).unwrap_or(u16::MAX),
                line: Some(content.rendered.line),
                border_cells: content.rendered.border_cells,
            });
        }
        GuideOverlay::new(body, overlay).render(buffer);
        self.conversation.targets.replace(targets);
    }

    pub(in crate::conversation) fn conversation_rows(
        &self,
        width: u16,
        palette: Palette,
    ) -> Vec<CommentRow> {
        let number_width = self
            .conversation
            .original
            .as_ref()
            .map_or(0, |code| code.number_width);
        let mut output = ConversationRows::new(width, number_width, palette);
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
        output.rows.extend(
            self.comments
                .conversation_rows(thread, output.frame, palette),
        );
        output.rule(FrameRule::Bottom);
        for row in &mut output.rows {
            if row.rendered.border_cells.is_empty() {
                row.rendered.border_cells = output.frame.enclose_line(&mut row.rendered.line);
            }
        }
        output.rows
    }

    pub(in crate::conversation) fn conversation_scroll_limit(&self) -> usize {
        let rows = self.conversation_rows(self.viewport_width, self.palette);
        ConversationViewport::new(rows, usize::from(self.viewport_height)).scroll_limit()
    }

    fn context_rows(&self, thread: &ReviewThread, palette: Palette, output: &mut ConversationRows) {
        output.file_link(thread.path(), palette);
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
