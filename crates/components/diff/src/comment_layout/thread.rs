use comment_editor::CommentEditor;
use diff_rendering::{DiffFrame, FrameRule};
use markdown_rendering::MarkdownRenderer;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use review_threads::{AskedUnder, Author, Message, MessageId, ReviewThread};
use ui_frame::Frame;
use ui_theme::Palette;
use unicode_width::UnicodeWidthStr;

use super::CommentRow;
use super::controls::DraftControls;
use crate::comments::{CommentTarget, Comments};
use review_drafts::{DraftId, OpenDraft};

#[derive(Clone, Copy)]
pub(super) struct ThreadLayout {
    pub(super) frame: DiffFrame,
    pub(super) source_row: usize,
    pub(super) palette: Palette,
    pub(super) outdated: bool,
}

impl ThreadLayout {
    pub(super) fn new(frame: DiffFrame, source_row: usize, palette: Palette) -> Self {
        Self {
            frame,
            source_row,
            palette,
            outdated: false,
        }
    }
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
        let drafts = self.drafts();
        let replying = self
            .focused_id()
            .filter(|_| self.replies_to(thread))
            .and_then(|id| drafts.get(id).map(|open| (id, open)));
        if let Some(editing) = replying {
            layout.separator(&mut rows, None);
            self.editor_rows(&mut rows, editing, true, layout);
        } else if let Some(parked) = self.parked_reply(&drafts, &thread.id) {
            layout.separator(&mut rows, None);
            self.editor_rows(&mut rows, parked, false, layout);
        } else if let Some(comment) = thread.messages.last() {
            layout.separator(&mut rows, Some(&comment.id));
            if thread.round().is_some() {
                layout.round_hint(&mut rows, &comment.id);
            } else {
                layout.reply_field(&mut rows, &comment.id);
            }
            layout.controls(&mut rows, Some(thread), None);
        }
        rows
    }

    /// Only the focused editor takes keys; the others keep their text and reopen on click.
    pub(super) fn editor_rows(
        &self,
        rows: &mut Vec<CommentRow>,
        (draft_id, editing): (DraftId, &OpenDraft),
        focused: bool,
        layout: ThreadLayout,
    ) {
        let (frame, palette) = (layout.frame, layout.palette);
        let start = rows.len();
        let mut buttons = Vec::new();
        let thread = editing
            .draft()
            .reply_to
            .as_ref()
            .and_then(|reply_to| self.book()?.thread_for_message(reply_to));
        let draft = DraftControls {
            draft: draft_id,
            focused,
        };
        layout.controls(&mut buttons, thread, Some(draft));
        let extra_button_rows = u16::try_from(buttons.len().saturating_sub(1)).unwrap_or(u16::MAX);
        let id = None;
        let status = if editing.is_posting() {
            "Posting…"
        } else if frame.content_width() < 24 {
            ""
        } else if focused {
            "Ctrl-Enter post"
        } else {
            "Draft · click to edit"
        };
        layout.header(rows, Author::Reviewer, status, id);
        layout.field_border(rows, true, focused, id);
        let area = Rect::new(
            0,
            0,
            frame.content_width().max(1),
            self.editor_height()
                .saturating_sub(extra_button_rows)
                .max(1),
        );
        let mut buffer = Buffer::empty(area);
        editing.editor().render(area, &mut buffer, palette, focused);
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
            layout.field_line(rows, Line::from(spans), focused, id);
        }
        if focused {
            layout.editor_border(rows, editing.editor(), id);
        } else {
            layout.field_border(rows, false, false, id);
        }
        let target = (!focused).then_some(CommentTarget::Draft(draft_id));
        for row in &mut rows[start..] {
            row.editor = focused;
            row.target.clone_from(&target);
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
            "original context".to_owned()
        } else {
            message
                .asked_under
                .as_ref()
                .map(Self::asked_under)
                .unwrap_or_default()
        };
        self.blank(rows, id);
        self.header(rows, message.author, &status, id);
        self.blank(rows, id);
        if show_excerpt && let Some(code) = thread.code() {
            self.plain(rows, &code.excerpt, self.palette.dim, id);
            self.blank(rows, id);
        }
        if let Some(quote) = &message.quote {
            let quoted = quote
                .lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            self.plain(rows, &quoted, self.palette.dim, id);
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

    /// Where in its Explore round the reviewer wrote a message of a round conversation.
    fn asked_under(asked_under: &AskedUnder) -> String {
        match asked_under {
            AskedUnder::Question {
                number: Some(number),
                ..
            } => format!("under {number}"),
            AskedUnder::Question {
                question, version, ..
            } => format!("under question {question}, version {version}"),
            AskedUnder::Design => "under the design".into(),
            AskedUnder::Conclusion { .. } => "under the conclusion".into(),
        }
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
        let border = self.border_style(active);
        let rule = Line::styled("─".repeat(self.border_width()), border);
        self.framed_border(rows, top, rule, border, id);
    }

    /// Close an active editor field with its mode and keymap hint in the border.
    fn editor_border(
        self,
        rows: &mut Vec<CommentRow>,
        editor: &CommentEditor,
        id: Option<&MessageId>,
    ) {
        let border = self.border_style(true);
        let width = u16::try_from(self.border_width()).unwrap_or(u16::MAX);
        let status = editor.status_border(width, border, self.palette);
        self.framed_border(rows, false, status, border, id);
    }

    fn border_width(self) -> usize {
        usize::from(self.frame.content_width()) + 2
    }

    fn border_style(self, active: bool) -> Style {
        Frame::Pane { focused: active }.border_style(self.palette)
    }

    fn framed_border(
        self,
        rows: &mut Vec<CommentRow>,
        top: bool,
        mut middle: Line<'static>,
        border: Style,
        id: Option<&MessageId>,
    ) {
        let (left, right) = if top { ('╭', '╮') } else { ('╰', '╯') };
        middle
            .spans
            .insert(0, Span::styled(left.to_string(), border));
        middle.spans.push(Span::styled(right.to_string(), border));
        Self {
            frame: self.frame.with_padding(0),
            ..self
        }
        .line(rows, middle, id);
    }

    fn field_line(
        self,
        rows: &mut Vec<CommentRow>,
        mut line: Line<'static>,
        active: bool,
        id: Option<&MessageId>,
    ) {
        let border = self.border_style(active);
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

    /// The reviewer writes in a round conversation from the Explore page, beside the question.
    fn round_hint(self, rows: &mut Vec<CommentRow>, id: &MessageId) {
        self.blank(rows, Some(id));
        self.plain(
            rows,
            "Write in this conversation from the Explore page.",
            self.palette.dim,
            Some(id),
        );
        self.blank(rows, Some(id));
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
