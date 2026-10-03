//! What the template receives of the question's citations: each with its note and the cited
//! lines of the change, colored on the server.

use review_explore_citations::{Citation, CodeColors, HighlightedRow, Token};
use review_repository::diff::DiffRow;
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct CitationContext<'a> {
    /// The path, side and lines: `src/main.rs new 7-9`.
    location: String,
    notes: &'a str,
    rows: Vec<RowContext<'a>>,
    /// Why the citation shows no rows.
    limitation: Option<String>,
}

#[derive(Serialize)]
struct RowContext<'a> {
    /// `context`, `added` or `removed`.
    kind: &'static str,
    old_line: Option<u32>,
    new_line: Option<u32>,
    tokens: Vec<TokenContext<'a>>,
}

#[derive(Serialize)]
struct TokenContext<'a> {
    text: &'a str,
    /// The palette role of the token's color, or `None` for the page's text color.
    role: Option<&'static str>,
}

impl<'a> CitationContext<'a> {
    pub(crate) fn new(citation: &'a Citation) -> Self {
        let (rows, limitation) = match &citation.lines {
            Ok(rows) => (rows.iter().filter_map(RowContext::new).collect(), None),
            Err(limitation) => (Vec::new(), Some(limitation.to_string())),
        };
        Self {
            location: citation.evidence.location.to_string(),
            notes: &citation.evidence.notes,
            rows,
            limitation,
        }
    }
}

impl<'a> RowContext<'a> {
    fn new(row: &'a HighlightedRow) -> Option<Self> {
        let (kind, old_line, new_line) = match row.diff {
            DiffRow::Context {
                old_line, new_line, ..
            } => ("context", Some(old_line), Some(new_line)),
            DiffRow::Delete { old_line, .. } => ("removed", Some(old_line), None),
            DiffRow::Add { new_line, .. } => ("added", None, Some(new_line)),
            _ => return None,
        };
        Some(Self {
            kind,
            old_line,
            new_line,
            tokens: row.tokens.iter().map(TokenContext::new).collect(),
        })
    }
}

impl<'a> TokenContext<'a> {
    fn new(token: &'a Token) -> Self {
        Self {
            text: &token.text,
            role: CodeColors::role(token),
        }
    }
}
