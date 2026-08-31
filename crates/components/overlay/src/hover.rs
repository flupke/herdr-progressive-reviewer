use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Widget};
use ratatui_markdown::markdown::{MarkdownRenderer, RenderHooks};
use ratatui_markdown::theme::ThemeConfig;
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
        let theme = ThemeConfig::default()
            .with_text_color(palette.text)
            .with_muted_text_color(palette.dim)
            .with_primary_color(palette.focus)
            .with_secondary_color(palette.focus)
            .with_info_color(palette.focus)
            .with_accent_yellow(palette.warning);
        let renderer = MarkdownRenderer::new(usize::from(inner.width))
            .with_render_hooks(Box::new(MarkdownCode(self.highlighter.clone())));
        Paragraph::new(renderer.render(&renderer.parse(markdown), &theme))
            .scroll((self.scroll, 0))
            .render(inner, buffer);
    }
}

struct MarkdownCode(SyntaxHighlighter);

impl RenderHooks for MarkdownCode {
    fn render_code_block(&self, language: &str, content: &str) -> Option<Vec<Line<'static>>> {
        Some(
            self.0
                .highlight_snippet(language, content)
                .into_iter()
                .map(|tokens| {
                    Line::from(
                        tokens
                            .into_iter()
                            .map(|token| Span::styled(token.text, Style::default().fg(token.color)))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect(),
        )
    }
}
