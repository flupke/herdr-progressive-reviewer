//! Complete code frames sized before the containing conversation wraps its rows.

use ratatui_legacy::style::{Modifier, Style};
use ratatui_legacy::text::{Line, Span};
use ratatui_markdown::markdown::{MarkdownBlock, MarkdownRenderer, RenderHooks};
use syntax_highlighting::SyntaxHighlighter;
use ui_theme::Palette;
use unicode_width::UnicodeWidthStr;

use super::MarkdownStyles;

pub(super) struct CodeBlocks {
    width: u16,
    palette: Palette,
    highlighter: Option<SyntaxHighlighter>,
}

impl CodeBlocks {
    pub(super) fn new(
        width: u16,
        palette: Palette,
        highlighter: Option<SyntaxHighlighter>,
    ) -> Self {
        Self {
            width: width.max(1),
            palette,
            highlighter,
        }
    }

    fn border_style(&self) -> Style {
        Style::default().fg(MarkdownStyles::legacy_color(self.palette.dim))
    }

    fn code(&self, language: &str, content: &str) -> Vec<Line<'static>> {
        if let Some(highlighter) = &self.highlighter {
            highlighter
                .highlight_snippet(language, content)
                .into_iter()
                .map(|tokens| {
                    Line::from(
                        tokens
                            .into_iter()
                            .map(|token| {
                                Span::styled(
                                    token.text,
                                    Style::default().fg(MarkdownStyles::legacy_color(token.color)),
                                )
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect()
        } else {
            let style = Style::default().fg(MarkdownStyles::legacy_color(self.palette.warning));
            content
                .lines()
                .map(|line| Line::styled(line.to_owned(), style))
                .collect()
        }
    }

    fn wrap(line: &Line<'_>, width: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut current = Line::default();
        let mut used = 0;
        for grapheme in line.styled_graphemes(Style::default()) {
            let symbol = if grapheme.symbol.width() > width {
                "�"
            } else {
                grapheme.symbol
            };
            let cells = symbol.width();
            if used + cells > width {
                lines.push(std::mem::take(&mut current));
                used = 0;
            }
            if let Some(span) = current.spans.last_mut()
                && span.style == grapheme.style
            {
                span.content.to_mut().push_str(symbol);
            } else {
                current
                    .spans
                    .push(Span::styled(symbol.to_owned(), grapheme.style));
            }
            used += cells;
        }
        lines.push(current);
        lines
    }

    fn rendered_width(line: &Line<'_>) -> usize {
        // Ratatui places graphemes separately, even when the full string forms a ligature.
        line.styled_graphemes(Style::default())
            .map(|grapheme| grapheme.symbol.width())
            .sum()
    }

    fn header(&self, language: &str) -> Line<'static> {
        let available = usize::from(self.width) - 3;
        let label = if language.is_empty() {
            Line::default()
        } else {
            Self::wrap(&Line::raw(format!(" {language} ")), available).remove(0)
        };
        Line::styled(
            format!(
                "╭─{label}{}╮",
                "─".repeat(available - Self::rendered_width(&label))
            ),
            self.border_style(),
        )
    }

    fn enclose(&self, mut line: Line<'static>) -> Line<'static> {
        let padding = usize::from(self.width) - 4 - Self::rendered_width(&line);
        line.spans
            .insert(0, Span::styled("│ ", self.border_style()));
        line.spans.push(Span::styled(
            format!("{} │", " ".repeat(padding)),
            self.border_style(),
        ));
        line
    }
}

impl RenderHooks for CodeBlocks {
    fn render_code_block(&self, language: &str, content: &str) -> Option<Vec<Line<'static>>> {
        // Leave room for at least one double-width glyph inside a padded frame.
        let framed = self.width >= 6;
        let inner = usize::from(self.width) - if framed { 4 } else { 0 };
        let mut lines = Vec::new();
        if framed {
            lines.push(self.header(language));
        }
        for line in self.code(language, content) {
            lines.extend(
                Self::wrap(&line, inner)
                    .into_iter()
                    .map(|line| if framed { self.enclose(line) } else { line }),
            );
        }
        if framed {
            lines.push(Line::styled(
                format!("╰{}╯", "─".repeat(usize::from(self.width) - 2)),
                self.border_style(),
            ));
        }
        Some(lines)
    }

    fn blockquote(&self, level: u8, children: &[MarkdownBlock]) -> Option<Vec<Line<'static>>> {
        let indent = (u16::from(level) * 2).min(self.width.saturating_sub(1));
        let inner = self.width - indent;
        let renderer = MarkdownRenderer::new(usize::from(inner)).with_render_hooks(Box::new(
            Self::new(inner, self.palette, self.highlighter.clone()),
        ));
        let theme = MarkdownStyles::new(self.palette).theme();
        let prefix = "│ "
            .chars()
            .cycle()
            .take(usize::from(indent))
            .collect::<String>();
        let mut lines = renderer.render(children, &theme);
        for line in &mut lines {
            for span in &mut line.spans {
                span.style = span
                    .style
                    .patch(self.border_style())
                    .add_modifier(Modifier::ITALIC);
            }
            line.spans
                .insert(0, Span::styled(prefix.clone(), self.border_style()));
        }
        Some(lines)
    }
}
