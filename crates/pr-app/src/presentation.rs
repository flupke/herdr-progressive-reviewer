//! Visible rows derived from a parsed unified diff.

use std::ops::RangeInclusive;

use pr_core::diff::DiffRow;
use pr_core::excerpt::{DiffExcerpt, ExcerptError};

use crate::highlight::{HighlightedRow, Token};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RowDisplay {
    Unified,
    FileContent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PresentedRow {
    pub(crate) source: usize,
    pub(crate) tokens: Vec<Token>,
    pub(crate) display: RowDisplay,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DiffPresentation {
    source: Vec<DiffRow>,
    pub(crate) rows: Vec<PresentedRow>,
}

impl DiffPresentation {
    pub(crate) fn new(highlighted: Vec<HighlightedRow>) -> Self {
        let added = highlighted.iter().any(|row| {
            matches!(
                &row.diff,
                DiffRow::Meta { text } if text.starts_with("new file mode ")
            )
        });
        let display = if added {
            RowDisplay::FileContent
        } else {
            RowDisplay::Unified
        };
        let mut source = Vec::with_capacity(highlighted.len());
        let mut rows = Vec::with_capacity(highlighted.len());
        for highlighted in highlighted {
            let visible = !added || matches!(highlighted.diff, DiffRow::Add { .. });
            source.push(highlighted.diff);
            if visible {
                rows.push(PresentedRow {
                    source: source.len() - 1,
                    tokens: highlighted.tokens,
                    display,
                });
            }
        }
        Self { source, rows }
    }

    pub(crate) fn row(&self, presented: &PresentedRow) -> &DiffRow {
        &self.source[presented.source]
    }

    pub(crate) fn has_notice(&self) -> bool {
        self.source
            .iter()
            .any(|row| matches!(row, DiffRow::Notice { .. }))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.source.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn is_selectable(&self, index: usize) -> bool {
        self.rows.get(index).is_some_and(|row| {
            matches!(
                self.row(row),
                DiffRow::Context { .. } | DiffRow::Delete { .. } | DiffRow::Add { .. }
            )
        })
    }

    pub(crate) fn excerpt(
        &self,
        selection: RangeInclusive<usize>,
    ) -> Result<DiffExcerpt, ExcerptError> {
        let start = self.rows[*selection.start()].source;
        let end = self.rows[*selection.end()].source;
        DiffExcerpt::build(&self.source, start..=end)
    }
}

#[cfg(test)]
mod tests {
    use pr_core::diff::DiffRow;
    use ratatui::style::Color;

    use super::{DiffPresentation, RowDisplay};
    use crate::highlight::{HighlightedRow, Token};

    #[test]
    fn added_file_shows_only_file_contents() {
        let highlighted = vec![
            HighlightedRow {
                diff: DiffRow::Meta {
                    text: "new file mode 100644".to_owned(),
                },
                tokens: Vec::new(),
            },
            HighlightedRow {
                diff: DiffRow::Hunk {
                    old_start: 0,
                    old_count: 0,
                    new_start: 1,
                    new_count: 1,
                },
                tokens: Vec::new(),
            },
            HighlightedRow {
                diff: DiffRow::Add {
                    new_line: 1,
                    text: "+fn main() {}".to_owned(),
                },
                tokens: vec![Token {
                    text: "fn main() {}".to_owned(),
                    color: Color::White,
                }],
            },
        ];

        let presentation = DiffPresentation::new(highlighted);

        assert_eq!(presentation.rows.len(), 1);
        assert_eq!(presentation.rows[0].display, RowDisplay::FileContent);
        assert!(matches!(
            presentation.row(&presentation.rows[0]),
            DiffRow::Add { .. }
        ));
    }
}
