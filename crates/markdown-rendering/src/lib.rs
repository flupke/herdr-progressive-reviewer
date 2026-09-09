//! Shared Markdown rendering for documentation and review conversations.

use ratatui::text::Line;
use ratatui_markdown::markdown::MarkdownRenderer as LegacyRenderer;
use syntax_highlighting::SyntaxHighlighter;
use ui_theme::Palette;

mod compatibility;
use compatibility::MarkdownStyles;
mod code_blocks;
use code_blocks::CodeBlocks;

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
        let renderer = LegacyRenderer::new(usize::from(width.max(1))).with_render_hooks(Box::new(
            CodeBlocks::new(width, palette, self.highlighter.clone()),
        ));
        let theme = MarkdownStyles::new(palette).theme();
        renderer
            .render(&renderer.parse(text), &theme)
            .into_iter()
            .map(MarkdownStyles::line)
            .collect()
    }
}
