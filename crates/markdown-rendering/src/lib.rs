//! Shared Markdown rendering for documentation and review conversations.

use ratatui::text::Line;
use ratatui_markdown::markdown::{MarkdownRenderer as LegacyRenderer, RenderHooks};
use syntax_highlighting::SyntaxHighlighter;
use ui_theme::Palette;

mod compatibility;
use compatibility::MarkdownStyles;

#[cfg(test)]
mod tests;

/// Render Markdown into the application's Ratatui types.
#[derive(Default)]
pub struct MarkdownRenderer {
    highlighter: Option<SyntaxHighlighter>,
}

impl MarkdownRenderer {
    /// Use the active syntax theme for fenced code blocks.
    pub fn new(highlighter: SyntaxHighlighter) -> Self {
        Self {
            highlighter: Some(highlighter),
        }
    }

    pub fn render(&self, text: &str, width: u16, palette: Palette) -> Vec<Line<'static>> {
        let mut renderer = LegacyRenderer::new(usize::from(width.max(1)));
        if let Some(highlighter) = &self.highlighter {
            renderer = renderer.with_render_hooks(Box::new(MarkdownCode(highlighter.clone())));
        }
        let theme = MarkdownStyles::new(palette).theme();
        renderer
            .render(&renderer.parse(text), &theme)
            .into_iter()
            .map(MarkdownStyles::line)
            .collect()
    }
}

struct MarkdownCode(SyntaxHighlighter);

impl RenderHooks for MarkdownCode {
    fn render_code_block(
        &self,
        language: &str,
        content: &str,
    ) -> Option<Vec<ratatui_legacy::text::Line<'static>>> {
        Some(
            self.0
                .highlight_snippet(language, content)
                .into_iter()
                .map(|tokens| {
                    ratatui_legacy::text::Line::from(
                        tokens
                            .into_iter()
                            .map(|token| {
                                ratatui_legacy::text::Span::styled(
                                    token.text,
                                    ratatui_legacy::style::Style::default()
                                        .fg(MarkdownStyles::legacy_color(token.color)),
                                )
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect(),
        )
    }
}
