//! Syntax colors for source code and unified diffs.

use std::path::Path;
use std::sync::OnceLock;

use ratatui::style::Color;
use review_repository::diff::DiffRow;
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

/// One syntax-colored part of a source line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Token {
    pub(crate) text: String,
    pub(crate) color: Color,
}

impl Token {
    fn new(text: &str, color: Color) -> Self {
        Self {
            text: text.to_owned(),
            color,
        }
    }
}

/// One diff row and its syntax-colored code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HighlightedRow {
    pub(crate) diff: DiffRow,
    pub(crate) tokens: Vec<Token>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HighlightedFile {
    AfterChange(Vec<Vec<Token>>),
    BeforeChange(Vec<Vec<Token>>),
}

impl HighlightedFile {
    pub(crate) fn after_change_lines(&self) -> Option<&[Vec<Token>]> {
        match self {
            Self::AfterChange(lines) => Some(lines),
            Self::BeforeChange(_) => None,
        }
    }

    pub(crate) fn into_lines(self) -> Vec<Vec<Token>> {
        match self {
            Self::AfterChange(lines) | Self::BeforeChange(lines) => lines,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HighlightedDiff {
    pub(crate) rows: Vec<HighlightedRow>,
    pub(crate) file: Option<HighlightedFile>,
}

/// Applies the configured syntax colors to source code and diff rows.
#[derive(Clone)]
pub(crate) struct SyntaxHighlighter {
    theme: Theme,
    plain: Color,
}

impl std::fmt::Debug for SyntaxHighlighter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SyntaxHighlighter")
            .finish_non_exhaustive()
    }
}

impl SyntaxHighlighter {
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
    ) -> HighlightedDiff {
        let old = self.highlight_file(path, old_content);
        let new = self.highlight_file(path, new_content);
        let rows = rows
            .into_iter()
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
            .collect();
        let file = if new_content.is_some() {
            new.map(HighlightedFile::AfterChange)
        } else {
            old.map(HighlightedFile::BeforeChange)
        };
        HighlightedDiff { rows, file }
    }

    pub(crate) fn highlight_snippet(&self, language: &str, content: &str) -> Vec<Vec<Token>> {
        self.highlight_file(language, Some(content.as_bytes()))
            .expect("a string is valid UTF-8")
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
        let Some(syntax) = Self::syntax(path) else {
            return Some(
                LinesWithEndings::from(content)
                    .map(|line| vec![Token::new(line.trim_end_matches(['\r', '\n']), self.plain)])
                    .collect(),
            );
        };
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
                    (!text.is_empty()).then(|| {
                        Token::new(
                            text,
                            Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b),
                        )
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
        vec![Token::new(
            text.strip_prefix([' ', '-', '+']).unwrap_or(text),
            self.plain,
        )]
    }
}

#[cfg(test)]
#[path = "highlight.tests.rs"]
mod tests;
