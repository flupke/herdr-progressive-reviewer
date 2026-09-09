use guide_rendering::{DiffFrame, FrameRule};
use markdown_rendering::MarkdownRenderer;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use review_threads::{Author, Message, MessageId, ReviewThread};
use ui_theme::Palette;
use unicode_width::UnicodeWidthStr;

use super::CommentRow;
use crate::comments::{CommentTarget, Comments};

#[derive(Clone, Copy)]
pub(super) struct ThreadLayout {
    pub(super) frame: DiffFrame,
    pub(super) source_row: usize,
    pub(super) palette: Palette,
    pub(super) outdated: bool,
}

impl Comments {
    pub(super) fn thread_rows(
        &self,
        thread: &ReviewThread,
        layout: ThreadLayout,
    ) -> Vec<CommentRow> {
        let mut rows = Vec::new();
        for (index, comment) in thread.messages.iter().enumerate() {
            if index > 0 {
                layout.separator(&mut rows, Some(&comment.id));
            }
            layout.comment_rows(&mut rows, thread, comment, index == 0 && layout.outdated);
        }
        let replying = self.editing.as_ref().is_some_and(|editing| {
            thread
                .messages
                .iter()
                .any(|comment| editing.draft.reply_to.as_ref() == Some(&comment.id))
        });
        if replying {
            layout.separator(&mut rows, None);
            self.editor_rows(&mut rows, layout.source_row, layout.frame, layout.palette);
        } else if let Some(comment) = thread.messages.last() {
            layout.separator(&mut rows, Some(&comment.id));
            layout.reply_field(&mut rows, &comment.id);
            layout.controls(&mut rows, Some(thread), false);
        }
        rows
    }

    pub(super) fn editor_rows(
        &self,
        rows: &mut Vec<CommentRow>,
        source_row: usize,
        frame: DiffFrame,
        palette: Palette,
    ) {
        let Some(editing) = &self.editing else { return };
        let layout = ThreadLayout {
            frame,
            source_row,
            palette,
            outdated: false,
        };
        let start = rows.len();
        let mut buttons = Vec::new();
        let thread = editing.draft.reply_to.as_ref().and_then(|id| {
            self.book
                .as_ref()
                .and_then(|book| book.thread_for_message(id))
        });
        layout.controls(&mut buttons, thread, true);
        let extra_button_rows = u16::try_from(buttons.len().saturating_sub(1)).unwrap_or(u16::MAX);
        let id = None;
        let status = if editing.posting.is_some() {
            "Posting…"
        } else if frame.content_width() >= 24 {
            "Ctrl-Enter post"
        } else {
            ""
        };
        layout.header(rows, Author::Reviewer, status, id);
        layout.field_border(rows, true, true, id);
        let area = Rect::new(
            0,
            0,
            frame.content_width().max(1),
            self.editor_height.saturating_sub(extra_button_rows).max(1),
        );
        let mut buffer = Buffer::empty(area);
        editing.editor.render(area, &mut buffer, palette);
        for y in 0..area.height {
            let mut spans = Vec::new();
            let mut x = 0;
            while x < area.width {
                let cell = &buffer[(x, y)];
                spans.push(Span::styled(cell.symbol().to_owned(), cell.style()));
                x = x.saturating_add(
                    u16::try_from(cell.symbol().width().max(1)).unwrap_or(u16::MAX),
                );
            }
            layout.field_line(rows, Line::from(spans), true, id);
        }
        layout.field_border(rows, false, true, id);
        for row in &mut rows[start..] {
            row.editor = true;
            row.target = None;
        }
        rows.extend(buttons);
    }
}

impl ThreadLayout {
    fn comment_rows(
        self,
        rows: &mut Vec<CommentRow>,
        thread: &ReviewThread,
        message: &Message,
        show_excerpt: bool,
    ) {
        let id = Some(&message.id);
        let status = if self.outdated {
            "original context"
        } else {
            ""
        };
        self.blank(rows, id);
        self.header(rows, message.author, status, id);
        self.blank(rows, id);
        if show_excerpt {
            self.plain(rows, &thread.excerpt, self.palette.dim, id);
            self.blank(rows, id);
        }
        let body = rows.len();
        self.markdown(rows, &message.text, id);
        if message.author == Author::Agent {
            for row in &mut rows[body..] {
                row.reply = Some(message.id.clone());
            }
        }
        self.blank(rows, id);
    }

    fn style(self) -> Style {
        Style::default().fg(self.palette.text)
    }

    fn line(self, rows: &mut Vec<CommentRow>, line: Line<'static>, id: Option<&MessageId>) {
        let style = self.style().patch(line.style);
        rows.extend(
            self.frame
                .wrapped_content(&line.style(style), self.source_row)
                .into_iter()
                .map(|row| CommentRow::new(row, id)),
        );
    }

    fn blank(self, rows: &mut Vec<CommentRow>, id: Option<&MessageId>) {
        self.line(rows, Line::default(), id);
    }

    fn separator(self, rows: &mut Vec<CommentRow>, id: Option<&MessageId>) {
        rows.push(CommentRow::new(
            self.frame.rule(FrameRule::Middle, self.source_row),
            id,
        ));
    }

    fn header(
        self,
        rows: &mut Vec<CommentRow>,
        author: Author,
        status: &str,
        id: Option<&MessageId>,
    ) {
        let name = match author {
            Author::Reviewer => "You",
            Author::Agent => "Agent",
        };
        let mut spans = vec![Span::styled(
            name,
            Style::default().add_modifier(Modifier::BOLD),
        )];
        if !status.is_empty() {
            spans.push(Span::styled(
                format!("  {status}"),
                Style::default().fg(self.palette.dim),
            ));
        }
        self.line(rows, Line::from(spans), id);
    }

    fn plain(
        self,
        rows: &mut Vec<CommentRow>,
        text: &str,
        color: ratatui::style::Color,
        id: Option<&MessageId>,
    ) {
        for line in text.lines() {
            let line = line
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect::<String>();
            self.line(rows, Line::styled(line, Style::default().fg(color)), id);
        }
    }

    fn markdown(self, rows: &mut Vec<CommentRow>, text: &str, id: Option<&MessageId>) {
        let text = text
            .chars()
            .map(|c| {
                if c.is_control() && c != '\n' && c != '\t' {
                    ' '
                } else {
                    c
                }
            })
            .collect::<String>();
        let width = self.frame.content_width();
        let mut lines = MarkdownRenderer::default().render(&text, width, self.palette);
        while lines
            .last()
            .is_some_and(|line| line.to_string().trim().is_empty())
        {
            lines.pop();
        }
        for line in lines {
            self.line(rows, line, id);
        }
    }

    fn field_border(
        self,
        rows: &mut Vec<CommentRow>,
        top: bool,
        active: bool,
        id: Option<&MessageId>,
    ) {
        let (left, right) = if top { ('╭', '╮') } else { ('╰', '╯') };
        let width = usize::from(self.frame.content_width()) + 2;
        let color = if active {
            self.palette.focus
        } else {
            self.palette.dim
        };
        Self {
            frame: self.frame.with_padding(0),
            ..self
        }
        .line(
            rows,
            Line::styled(
                format!("{left}{}{right}", "─".repeat(width)),
                Style::default().fg(color),
            ),
            id,
        );
    }

    fn field_line(
        self,
        rows: &mut Vec<CommentRow>,
        mut line: Line<'static>,
        active: bool,
        id: Option<&MessageId>,
    ) {
        let border = Style::default().fg(if active {
            self.palette.focus
        } else {
            self.palette.dim
        });
        let mut spans = vec![Span::styled("│ ", border)];
        let padding = usize::from(self.frame.content_width()).saturating_sub(line.width());
        spans.append(&mut line.spans);
        spans.push(Span::raw(" ".repeat(padding)));
        spans.push(Span::styled(" │", border));
        Self {
            frame: self.frame.with_padding(0),
            ..self
        }
        .line(rows, Line::from(spans), id);
    }

    fn reply_field(self, rows: &mut Vec<CommentRow>, id: &MessageId) {
        let start = rows.len();
        self.blank(rows, Some(id));
        self.field_border(rows, true, false, Some(id));
        let line = Line::styled("Reply…", Style::default().fg(self.palette.dim));
        self.field_line(rows, line, false, Some(id));
        self.field_border(rows, false, false, Some(id));
        self.blank(rows, Some(id));
        for row in &mut rows[start..] {
            row.target = Some(CommentTarget::Reply(id.clone()));
        }
    }
}
