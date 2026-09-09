use markdown_rendering::MarkdownRenderer;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Paragraph, Widget};
use syntax_highlighting::SyntaxHighlighter;
use ui_shortcuts::Key;
use ui_theme::Palette;

use crate::popup::{centered_area, popup_block};

pub(super) struct HoverOverlay {
    markdown: Option<String>,
    scroll: u16,
    highlighter: SyntaxHighlighter,
}

impl HoverOverlay {
    pub(super) fn new(highlighter: SyntaxHighlighter) -> Self {
        Self {
            markdown: None,
            scroll: 0,
            highlighter,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.markdown.is_some()
    }

    pub(super) fn replace_markdown(&mut self, markdown: Option<&String>) {
        self.markdown = markdown.cloned();
        self.scroll = 0;
    }

    pub(super) fn close(&mut self) {
        self.markdown = None;
    }

    pub(super) fn handle_key(&mut self, key: Key) {
        match key {
            Key::Escape => self.close(),
            Key::Down | Key::Char('j') => self.scroll = self.scroll.saturating_add(1),
            Key::Up | Key::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            _ => {}
        }
    }

    pub(super) fn area(area: Rect) -> Rect {
        centered_area(
            area,
            area.width.saturating_mul(3) / 4,
            area.height.saturating_mul(3) / 4,
        )
    }

    pub(super) fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette) {
        let Some(markdown) = &self.markdown else {
            return;
        };
        let popup = Self::area(area);
        Clear.render(popup, buffer);
        let block = popup_block("Documentation · Esc close", palette);
        let inner = block.inner(popup);
        block.render(popup, buffer);
        let lines =
            MarkdownRenderer::new(self.highlighter.clone()).render(markdown, inner.width, palette);
        Paragraph::new(lines)
            .scroll((self.scroll, 0))
            .render(inner, buffer);
    }
}
