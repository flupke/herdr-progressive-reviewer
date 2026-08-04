use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};
use ratatui_markdown::markdown::{MarkdownRenderer, RenderHooks};
use ratatui_markdown::theme::ThemeConfig;

use crate::ReviewApp;
use crate::highlight::SyntaxHighlighter;

pub(super) struct HoverView<'a>(pub(super) &'a ReviewApp, pub(super) &'a str);

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

impl HoverView<'_> {
    fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let palette = self.0.palette;
        let theme = ThemeConfig::default()
            .with_text_color(palette.text)
            .with_muted_text_color(palette.dim)
            .with_primary_color(palette.focus)
            .with_secondary_color(palette.focus)
            .with_info_color(palette.focus)
            .with_accent_yellow(palette.warning);
        let renderer = MarkdownRenderer::new(usize::from(width))
            .with_render_hooks(Box::new(MarkdownCode(self.0.highlighter.clone())));
        renderer.render(&renderer.parse(self.1), &theme)
    }
}

impl Widget for HoverView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let width = area.width.saturating_mul(3) / 4;
        let height = area.height.saturating_mul(3) / 4;
        let popup = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        Clear.render(popup, buffer);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Documentation · Esc close ")
            .border_style(Style::default().fg(self.0.palette.focus));
        let inner = block.inner(popup);
        block.render(popup, buffer);
        Paragraph::new(self.lines(inner.width))
            .scroll((self.0.hover_scroll, 0))
            .render(inner, buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::{HoverView, ReviewApp};

    #[test]
    fn hover_hides_fences_and_uses_source_syntax_colors() {
        let app = ReviewApp::default();
        let markdown = "```rust\n\tpub struct Example;\n```";
        let lines = HoverView(&app, markdown).lines(80);
        let text = lines
            .iter()
            .flat_map(|line| &line.spans)
            .map(|span| span.content.as_ref())
            .collect::<String>();
        let expected = app
            .highlighter
            .highlight_snippet("rust", "pub struct Example;\n")[0][0]
            .color;
        let actual = lines
            .iter()
            .flat_map(|line| &line.spans)
            .find(|span| span.content == "pub")
            .and_then(|span| span.style.fg);

        assert!(!text.contains("```"));
        assert!(text.contains("    pub struct Example;"));
        assert_eq!(actual, Some(expected));
    }
}
