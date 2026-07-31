//! Visible rows derived from a parsed unified diff.

use std::ops::RangeInclusive;

use pr_core::diff::DiffRow;
use pr_core::excerpt::{DiffExcerpt, ExcerptError};

use crate::highlight::{HighlightedDiff, Token};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PresentedRow {
    Diff { source: usize, tokens: Vec<Token> },
    Gap { start: u32, lines: Vec<Vec<Token>> },
    Expanded { line: u32, tokens: Vec<Token> },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DiffPresentation {
    source: Vec<DiffRow>,
    pub(crate) rows: Vec<PresentedRow>,
}

impl DiffPresentation {
    pub(crate) fn new(highlighted: HighlightedDiff) -> Self {
        let added = highlighted.rows.iter().any(|row| {
            matches!(
                &row.diff,
                DiffRow::Meta { text } if text.starts_with("new file mode ")
            )
        });
        let mut source = Vec::with_capacity(highlighted.rows.len());
        let mut rows = Vec::with_capacity(highlighted.rows.len());
        let mut previous_hunk_end = None;
        for highlighted_row in highlighted.rows {
            let visible = if added {
                matches!(highlighted_row.diff, DiffRow::Add { .. })
            } else {
                matches!(
                    highlighted_row.diff,
                    DiffRow::Context { .. }
                        | DiffRow::Delete { .. }
                        | DiffRow::Add { .. }
                        | DiffRow::Notice { .. }
                )
            };
            if let DiffRow::Hunk {
                new_start,
                new_count,
                ..
            } = &highlighted_row.diff
            {
                if let Some(start) = previous_hunk_end
                    && *new_start > start
                    && let Some(lines) =
                        Self::gap_lines(highlighted.new_lines.as_deref(), start, *new_start)
                {
                    rows.push(PresentedRow::Gap { start, lines });
                }
                previous_hunk_end = Some(new_start.saturating_add(*new_count));
            }
            source.push(highlighted_row.diff);
            if visible {
                rows.push(PresentedRow::Diff {
                    source: source.len() - 1,
                    tokens: highlighted_row.tokens,
                });
            }
        }
        Self { source, rows }
    }

    fn gap_lines(lines: Option<&[Vec<Token>]>, start: u32, end: u32) -> Option<Vec<Vec<Token>>> {
        let first = usize::try_from(start.checked_sub(1)?).ok()?;
        let last = usize::try_from(end.checked_sub(1)?).ok()?;
        Some(lines?.get(first..last)?.to_vec())
    }

    pub(crate) fn source_row(&self, source: usize) -> &DiffRow {
        &self.source[source]
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
        self.rows.get(index).is_some_and(|row| match row {
            PresentedRow::Diff { source, .. } => matches!(
                self.source_row(*source),
                DiffRow::Context { .. } | DiffRow::Delete { .. } | DiffRow::Add { .. }
            ),
            PresentedRow::Gap { .. } | PresentedRow::Expanded { .. } => false,
        })
    }

    pub(crate) fn expand(&mut self, index: usize) -> bool {
        let Some(PresentedRow::Gap { start, lines }) = self.rows.get(index).cloned() else {
            return false;
        };
        self.rows.splice(
            index..=index,
            lines
                .into_iter()
                .enumerate()
                .map(|(offset, tokens)| PresentedRow::Expanded {
                    line: start.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX)),
                    tokens,
                }),
        );
        true
    }

    pub(crate) fn line_number_width(&self) -> usize {
        self.source
            .iter()
            .filter_map(|row| match row {
                DiffRow::Context { new_line, .. } | DiffRow::Add { new_line, .. } => {
                    Some(*new_line)
                }
                DiffRow::Delete { old_line, .. } => Some(*old_line),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            .to_string()
            .len()
    }

    pub(crate) fn excerpt(
        &self,
        selection: RangeInclusive<usize>,
    ) -> Result<DiffExcerpt, ExcerptError> {
        let mut sources = self.rows[selection].iter().filter_map(|row| match row {
            PresentedRow::Diff { source, .. } => Some(*source),
            PresentedRow::Gap { .. } | PresentedRow::Expanded { .. } => None,
        });
        let start = sources.next().ok_or(ExcerptError::NoContent)?;
        let end = sources.next_back().unwrap_or(start);
        DiffExcerpt::build(&self.source, start..=end)
    }
}

#[cfg(test)]
mod tests {
    use pr_core::diff::DiffRow;
    use ratatui::style::Color;

    use super::{DiffPresentation, PresentedRow};
    use crate::highlight::{HighlightedDiff, HighlightedRow, Token};

    #[test]
    fn added_file_shows_only_file_contents() {
        let highlighted = HighlightedDiff {
            rows: vec![
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
            ],
            new_lines: None,
        };

        let presentation = DiffPresentation::new(highlighted);

        assert_eq!(presentation.rows.len(), 1);
        assert!(matches!(presentation.rows[0], PresentedRow::Diff { .. }));
    }

    #[test]
    fn hides_headers_and_expands_the_gap_between_hunks() {
        let row = |diff| HighlightedRow {
            diff,
            tokens: Vec::new(),
        };
        let mut presentation = DiffPresentation::new(HighlightedDiff {
            rows: vec![
                row(DiffRow::FileHeader {
                    old_path: None,
                    new_path: None,
                    text: "diff --git a/file b/file".to_owned(),
                }),
                row(DiffRow::Hunk {
                    old_start: 1,
                    old_count: 1,
                    new_start: 1,
                    new_count: 1,
                }),
                row(DiffRow::Context {
                    old_line: 1,
                    new_line: 1,
                    text: " first".to_owned(),
                }),
                row(DiffRow::Hunk {
                    old_start: 5,
                    old_count: 1,
                    new_start: 5,
                    new_count: 1,
                }),
                row(DiffRow::Context {
                    old_line: 5,
                    new_line: 5,
                    text: " fifth".to_owned(),
                }),
            ],
            new_lines: Some(
                (1..=5)
                    .map(|line| {
                        vec![Token {
                            text: format!("line {line}"),
                            color: Color::White,
                        }]
                    })
                    .collect(),
            ),
        });

        assert!(matches!(presentation.rows[1], PresentedRow::Gap { .. }));
        assert!(presentation.expand(1));
        assert_eq!(presentation.rows.len(), 5);
        assert!(matches!(
            presentation.rows[1],
            PresentedRow::Expanded { line: 2, .. }
        ));
    }
}
