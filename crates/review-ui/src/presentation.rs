//! Visible rows derived from a parsed unified diff.

use std::ops::Range;
use std::ops::RangeInclusive;

use review_repository::diff::DiffRow;
use review_repository::excerpt::{DiffExcerpt, ExcerptError};

use crate::highlight::{HighlightedDiff, HighlightedFile, Token};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PresentedRow {
    Diff { source: usize, tokens: Vec<Token> },
    Gap { start: u32, lines: Vec<Vec<Token>> },
    Expanded { line: u32, tokens: Vec<Token> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SearchDirection {
    Forward,
    Backward,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WholeFile {
    Added,
    Deleted,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum PresentationView {
    #[default]
    Diff,
    File {
        diff_rows: Vec<PresentedRow>,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DiffPresentation {
    source: Vec<DiffRow>,
    pub(crate) rows: Vec<PresentedRow>,
    whole_file: Option<WholeFile>,
    file_rows: Option<Vec<PresentedRow>>,
    view: PresentationView,
}

impl DiffPresentation {
    pub(crate) fn new(highlighted: HighlightedDiff) -> Self {
        let HighlightedDiff {
            rows: highlighted_rows,
            file,
        } = highlighted;
        let after_change_lines = file.as_ref().and_then(HighlightedFile::after_change_lines);
        let whole_file = highlighted_rows.iter().find_map(|row| match &row.diff {
            DiffRow::Meta { text } if text.starts_with("new file mode ") => Some(WholeFile::Added),
            DiffRow::Meta { text } if text.starts_with("deleted file mode ") => {
                Some(WholeFile::Deleted)
            }
            _ => None,
        });
        let mut source = Vec::with_capacity(highlighted_rows.len());
        let mut rows = Vec::with_capacity(highlighted_rows.len());
        let mut previous_hunk_end = None;
        for highlighted_row in highlighted_rows {
            let visible = match whole_file {
                Some(WholeFile::Added) => matches!(highlighted_row.diff, DiffRow::Add { .. }),
                Some(WholeFile::Deleted) => matches!(highlighted_row.diff, DiffRow::Delete { .. }),
                None => matches!(
                    highlighted_row.diff,
                    DiffRow::Context { .. }
                        | DiffRow::Delete { .. }
                        | DiffRow::Add { .. }
                        | DiffRow::Notice { .. }
                ),
            };
            if let DiffRow::Hunk {
                new_start,
                new_count,
                ..
            } = &highlighted_row.diff
            {
                let start = previous_hunk_end.unwrap_or(1);
                if *new_start > start
                    && let Some(lines) = Self::gap_lines(after_change_lines, start, *new_start)
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
        if let Some(start) = previous_hunk_end
            && let Some(end) = after_change_lines
                .and_then(|lines| u32::try_from(lines.len()).ok())
                .map(|last| last.saturating_add(1))
            && end > start
            && let Some(lines) = Self::gap_lines(after_change_lines, start, end)
        {
            rows.push(PresentedRow::Gap { start, lines });
        }
        let file_rows = file.map(HighlightedFile::into_lines).map(|lines| {
            lines
                .into_iter()
                .enumerate()
                .map(|(index, tokens)| PresentedRow::Expanded {
                    line: u32::try_from(index + 1).unwrap_or(u32::MAX),
                    tokens,
                })
                .collect()
        });
        Self {
            source,
            rows,
            whole_file,
            file_rows,
            view: PresentationView::Diff,
        }
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

    pub(crate) fn shows_whole_file(&self) -> bool {
        self.whole_file.is_some() || self.is_file_view()
    }

    pub(crate) fn can_show_file(&self) -> bool {
        self.file_rows.is_some() || self.is_file_view()
    }

    pub(crate) fn is_file_view(&self) -> bool {
        matches!(self.view, PresentationView::File { .. })
    }

    pub(crate) fn show_file(&mut self) -> bool {
        if self.is_file_view() {
            return false;
        }
        let Some(rows) = self.file_rows.take() else {
            return false;
        };
        self.view = PresentationView::File {
            diff_rows: std::mem::replace(&mut self.rows, rows),
        };
        true
    }

    pub(crate) fn show_diff(&mut self) -> bool {
        let PresentationView::File { diff_rows } = std::mem::take(&mut self.view) else {
            return false;
        };
        self.file_rows = Some(std::mem::replace(&mut self.rows, diff_rows));
        true
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

    pub(crate) fn source_position(&self, index: usize) -> Option<(u32, String)> {
        let (line, tokens) = match self.rows.get(index)? {
            PresentedRow::Diff { source, tokens } => match self.source_row(*source) {
                DiffRow::Context { new_line, .. } | DiffRow::Add { new_line, .. } => {
                    (new_line.checked_sub(1)?, tokens)
                }
                DiffRow::Delete { .. }
                | DiffRow::Notice { .. }
                | DiffRow::FileHeader { .. }
                | DiffRow::Meta { .. }
                | DiffRow::Hunk { .. } => return None,
            },
            PresentedRow::Expanded { line, tokens }
                if self.whole_file != Some(WholeFile::Deleted) =>
            {
                (line.checked_sub(1)?, tokens)
            }
            PresentedRow::Gap { .. } | PresentedRow::Expanded { .. } => return None,
        };
        Some((
            line,
            tokens.iter().map(|token| token.text.as_str()).collect(),
        ))
    }

    pub(crate) fn reveal_line(&mut self, line: u32) -> Option<usize> {
        if let Some(index) = self.reveal_diff_line(line) {
            return Some(index);
        }
        let display_line = line.saturating_add(1);
        if self.show_file() {
            return self.rows.iter().position(
                |row| matches!(row, PresentedRow::Expanded { line, .. } if *line == display_line),
            );
        }
        None
    }

    pub(crate) fn reveal_diff_line(&mut self, line: u32) -> Option<usize> {
        let display_line = line.saturating_add(1);
        if let Some(index) = self.rows.iter().position(|row| match row {
            PresentedRow::Diff { source, .. } => matches!(
                self.source_row(*source),
                DiffRow::Context { new_line, .. } | DiffRow::Add { new_line, .. }
                    if *new_line == display_line
            ),
            PresentedRow::Expanded { line, .. } => *line == display_line,
            PresentedRow::Gap { .. } => false,
        }) {
            return Some(index);
        }
        if let Some(index) = self.rows.iter().position(|row| match row {
            PresentedRow::Gap { start, lines } => {
                display_line >= *start
                    && display_line
                        < start.saturating_add(u32::try_from(lines.len()).unwrap_or(u32::MAX))
            }
            PresentedRow::Diff { .. } | PresentedRow::Expanded { .. } => false,
        }) {
            self.expand(index);
            return self.rows.iter().position(
                |row| matches!(row, PresentedRow::Expanded { line, .. } if *line == display_line),
            );
        }
        None
    }

    pub(crate) fn find_matching_row(
        &self,
        query: &str,
        from: usize,
        direction: SearchDirection,
    ) -> Option<usize> {
        let len = self.rows.len();
        if query.is_empty() || len == 0 {
            return None;
        }
        let from = from.min(len - 1);
        match direction {
            SearchDirection::Backward => self
                .matching_rows(query)
                .rev()
                .find(|index| *index < from)
                .or_else(|| self.matching_rows(query).rev().find(|index| *index >= from)),
            SearchDirection::Forward => self
                .matching_rows(query)
                .find(|index| *index > from)
                .or_else(|| self.matching_rows(query).find(|index| *index <= from)),
        }
    }

    pub(crate) fn matching_rows<'a>(
        &'a self,
        query: &'a str,
    ) -> impl DoubleEndedIterator<Item = usize> + 'a {
        self.rows
            .iter()
            .enumerate()
            .filter_map(move |(index, _)| self.row_contains(index, query).then_some(index))
    }

    fn row_contains(&self, index: usize, query: &str) -> bool {
        let tokens = match &self.rows[index] {
            PresentedRow::Diff { tokens, .. } | PresentedRow::Expanded { tokens, .. } => tokens,
            PresentedRow::Gap { .. } => return false,
        };
        let text = tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect::<String>();
        !matching_ranges(&text, query).is_empty()
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

    pub(crate) fn expand_all(&mut self) -> bool {
        let mut changed = false;
        let mut index = 0;
        while index < self.rows.len() {
            changed |= self.expand(index);
            index += 1;
        }
        changed
    }

    pub(crate) fn contract_all(&mut self) -> bool {
        if !self
            .rows
            .iter()
            .any(|row| matches!(row, PresentedRow::Expanded { .. }))
        {
            return false;
        }
        let mut rows = Vec::with_capacity(self.rows.len());
        for row in std::mem::take(&mut self.rows) {
            let PresentedRow::Expanded { line, tokens } = row else {
                rows.push(row);
                continue;
            };
            if let Some(PresentedRow::Gap { start, lines }) = rows.last_mut()
                && start.saturating_add(u32::try_from(lines.len()).unwrap_or(u32::MAX)) == line
            {
                lines.push(tokens);
            } else {
                rows.push(PresentedRow::Gap {
                    start: line,
                    lines: vec![tokens],
                });
            }
        }
        self.rows = rows;
        true
    }

    pub(crate) fn line_number_width(&self) -> usize {
        let source_line = self
            .source
            .iter()
            .filter_map(|row| match row {
                DiffRow::Context { new_line, .. } | DiffRow::Add { new_line, .. } => {
                    Some(*new_line)
                }
                DiffRow::Delete { old_line, .. } => Some(*old_line),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        self.rows
            .iter()
            .filter_map(|row| match row {
                PresentedRow::Expanded { line, .. } => Some(*line),
                PresentedRow::Diff { .. } | PresentedRow::Gap { .. } => None,
            })
            .max()
            .unwrap_or(source_line)
            .max(source_line)
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

pub(crate) fn matching_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    if query.chars().any(char::is_uppercase) {
        return text
            .match_indices(query)
            .map(|(start, found)| start..start + found.len())
            .collect();
    }
    if text.is_ascii() && query.is_ascii() {
        let mut matches = Vec::new();
        let mut start = 0;
        while start + query.len() <= text.len() {
            if text.as_bytes()[start..start + query.len()].eq_ignore_ascii_case(query.as_bytes()) {
                matches.push(start..start + query.len());
                start += query.len();
            } else {
                start += 1;
            }
        }
        return matches;
    }

    let query = query.to_lowercase();
    let boundaries = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .collect::<Vec<_>>();
    let mut matches = Vec::new();
    let mut start = 0;
    // ponytail: this is quadratic for non-ASCII text; replace it if long Unicode lines stutter.
    while start + 1 < boundaries.len() {
        let Some(end) = (start + 1..boundaries.len())
            .find(|end| text[boundaries[start]..boundaries[*end]].to_lowercase() == query)
        else {
            start += 1;
            continue;
        };
        matches.push(boundaries[start]..boundaries[end]);
        start = end;
    }
    matches
}

#[cfg(test)]
#[path = "presentation.tests.rs"]
mod tests;
