//! Syntax colors for unified diff code.

use std::path::Path;
use std::sync::OnceLock;

use pr_core::diff::DiffRow;
use ratatui::style::Color;
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

/// One syntax-colored part of a diff row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Token {
    pub(crate) text: String,
    pub(crate) color: Color,
}

/// One diff row and its syntax-colored code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HighlightedRow {
    pub(crate) diff: DiffRow,
    pub(crate) tokens: Vec<Token>,
}

/// Highlights diff rows from complete old and new file contents.
pub(crate) struct DiffHighlighter {
    theme: Theme,
    plain: Color,
}

impl std::fmt::Debug for DiffHighlighter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DiffHighlighter")
            .finish_non_exhaustive()
    }
}

impl DiffHighlighter {
    pub(crate) fn new(theme: crate::theme::Theme) -> Self {
        Self {
            theme: Self::themes().get(theme.syntax).clone(),
            plain: theme.palette.text,
        }
    }

    pub(crate) fn highlight(
        &self,
        path: &str,
        rows: Vec<DiffRow>,
        old_content: Option<&[u8]>,
        new_content: Option<&[u8]>,
    ) -> Vec<HighlightedRow> {
        let old = self.highlight_file(path, old_content);
        let new = self.highlight_file(path, new_content);
        rows.into_iter()
            .map(|diff| {
                let tokens = match &diff {
                    DiffRow::Context { new_line, text, .. } | DiffRow::Add { new_line, text } => {
                        Self::line_tokens(new.as_deref(), *new_line)
                            .unwrap_or_else(|| self.plain_diff_line(text))
                    }
                    DiffRow::Delete { old_line, text } => {
                        Self::line_tokens(old.as_deref(), *old_line)
                            .unwrap_or_else(|| self.plain_diff_line(text))
                    }
                    _ => Vec::new(),
                };
                HighlightedRow { diff, tokens }
            })
            .collect()
    }

    fn syntaxes() -> &'static SyntaxSet {
        static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
        SYNTAXES.get_or_init(two_face::syntax::extra_newlines)
    }

    fn themes() -> &'static two_face::theme::EmbeddedLazyThemeSet {
        static THEMES: OnceLock<two_face::theme::EmbeddedLazyThemeSet> = OnceLock::new();
        THEMES.get_or_init(two_face::theme::extra)
    }

    fn syntax(path: &str) -> Option<&'static SyntaxReference> {
        let path = Path::new(path);
        let file_name = path.file_name()?.to_str()?;
        let set = Self::syntaxes();
        set.find_syntax_by_extension(file_name)
            .or_else(|| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .and_then(|extension| set.find_syntax_by_extension(extension))
            })
            .or_else(|| set.find_syntax_by_token(file_name))
    }

    fn highlight_file(&self, path: &str, content: Option<&[u8]>) -> Option<Vec<Vec<Token>>> {
        let content = std::str::from_utf8(content?).ok()?;
        let syntax = Self::syntax(path)?;
        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        LinesWithEndings::from(content)
            .map(|line| Self::highlight_line(&mut highlighter, line))
            .collect()
    }

    fn highlight_line(highlighter: &mut HighlightLines<'_>, line: &str) -> Option<Vec<Token>> {
        Some(
            highlighter
                .highlight_line(line, Self::syntaxes())
                .ok()?
                .into_iter()
                .filter_map(|(style, text)| {
                    let text = text.trim_end_matches(['\r', '\n']);
                    (!text.is_empty()).then(|| Token {
                        text: text.to_owned(),
                        color: Color::Rgb(
                            style.foreground.r,
                            style.foreground.g,
                            style.foreground.b,
                        ),
                    })
                })
                .collect(),
        )
    }

    fn line_tokens(lines: Option<&[Vec<Token>]>, line: u32) -> Option<Vec<Token>> {
        let index = usize::try_from(line.checked_sub(1)?).ok()?;
        lines?.get(index).cloned()
    }

    fn plain_diff_line(&self, text: &str) -> Vec<Token> {
        vec![Token {
            text: text
                .strip_prefix([' ', '-', '+'])
                .unwrap_or(text)
                .to_owned(),
            color: self.plain,
        }]
    }
}

#[cfg(test)]
mod tests {
    use pr_core::diff::DiffRow;

    use super::DiffHighlighter;
    use crate::theme::Theme;

    #[test]
    fn uses_syntax_state_from_lines_before_the_hunk() {
        let rows = vec![DiffRow::Add {
            new_line: 2,
            text: "+let answer = 42;".to_owned(),
        }];
        let highlighted = DiffHighlighter::new(Theme::default()).highlight(
            "src/lib.rs",
            rows,
            None,
            Some(b"/* comment\nlet answer = 42;\n*/\n"),
        );

        assert_eq!(
            highlighted[0]
                .tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            "let answer = 42;"
        );
        assert_eq!(highlighted[0].tokens.len(), 1);
    }
}
