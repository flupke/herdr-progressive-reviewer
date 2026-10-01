//! Reviewed hunks folded into the open diff, and the marks each hunk offers.

use std::ops::Range;

use ratatui::style::Color;
use review_hunks::{FileHunks, HunkMark, LineCount, OpenHunk};
use review_repository::diff::DiffRow;
use syntax_highlighting::Token;
use ui_events::PresentationLocation;

use super::{DiffPresentation, PresentedRow};

/// The review control a hunk shows in its top-right corner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HunkBadge {
    /// An open hunk; `since_review` when it rewrites reviewed lines.
    Open { since_review: bool },
    /// A reviewed hunk.
    Reviewed,
}

impl DiffPresentation {
    /// Fold each reviewed hunk into one row at its place in the current file.
    pub(crate) fn with_hunks(mut self, hunks: FileHunks) -> Self {
        let lines = hunks
            .reviewed
            .iter()
            .map(|hunk| hunk.span.new.clone())
            .collect::<Vec<_>>();
        self.hunks = hunks;
        for (hunk, lines) in lines.into_iter().enumerate().rev() {
            self.fold_reviewed(hunk, &lines);
        }
        self
    }

    /// Replace the hunk's current lines, in unchanged sections or in an open
    /// hunk's context, with one folded row.
    fn fold_reviewed(&mut self, hunk: usize, lines: &Range<u32>) {
        // Rows number lines from one.
        let (first, end) = (lines.start + 1, lines.end + 1);
        let source = &self.source;
        let mut rows = Vec::with_capacity(self.rows.len() + 2);
        for row in std::mem::take(&mut self.rows) {
            match row {
                PresentedRow::Gap { start, lines } => {
                    rows.extend(split_gap(start, lines, first..end));
                }
                PresentedRow::Diff { source: index, .. }
                    if matches!(
                        source[index],
                        DiffRow::Context { new_line, .. } if (first..end).contains(&new_line)
                    ) => {}
                row => rows.push(row),
            }
        }
        self.rows = rows;
        let index = self
            .rows
            .iter()
            .position(|row| self.first_new_line(row).is_some_and(|line| line >= first))
            .unwrap_or(self.rows.len());
        self.rows.insert(index, PresentedRow::ReviewedHunk { hunk });
    }

    /// The folded reviewed hunk that holds a location's current line.
    pub(crate) fn fold_at(&self, location: PresentationLocation) -> Option<usize> {
        match location {
            PresentationLocation::NewLine(line)
            | PresentationLocation::Context { new_line: line, .. } => self.folded_row_holding(line),
            _ => None,
        }
    }

    /// The folded reviewed hunk that holds the zero-based current line.
    pub(crate) fn folded_row_holding(&self, line: u32) -> Option<usize> {
        self.rows.iter().position(|row| match row {
            PresentedRow::ReviewedHunk { hunk } => {
                let lines = &self.hunks.reviewed[*hunk].span.new;
                lines.contains(&line) || (lines.is_empty() && lines.start == line)
            }
            _ => false,
        })
    }

    fn first_new_line(&self, row: &PresentedRow) -> Option<u32> {
        match row {
            PresentedRow::Diff { source, .. } => match self.source_row(*source) {
                DiffRow::Context { new_line, .. } | DiffRow::Add { new_line, .. } => {
                    Some(*new_line)
                }
                _ => None,
            },
            PresentedRow::Gap { start, .. } => Some(*start),
            PresentedRow::Expanded { line, .. } => Some(*line),
            PresentedRow::ReviewedHunk { hunk } | PresentedRow::ReviewedLine { hunk, .. } => {
                Some(self.hunks.reviewed[*hunk].span.new.start + 1)
            }
        }
    }

    /// Show the rows of a folded reviewed hunk.
    pub(crate) fn expand_reviewed(&mut self, index: usize) -> bool {
        let Some(PresentedRow::ReviewedHunk { hunk }) = self.rows.get(index) else {
            return false;
        };
        let hunk = *hunk;
        let rows = self.hunks.reviewed[hunk]
            .rows
            .iter()
            .enumerate()
            .map(|(row, diff)| {
                let new_line = match diff {
                    DiffRow::Context { new_line, .. } | DiffRow::Add { new_line, .. } => {
                        Some(*new_line)
                    }
                    _ => None,
                };
                PresentedRow::ReviewedLine {
                    hunk,
                    row,
                    new_line,
                    tokens: new_line
                        .and_then(|line| self.file_line_tokens(line))
                        .unwrap_or_else(|| plain_tokens(diff)),
                }
            })
            .collect::<Vec<_>>();
        self.search_document.take();
        self.rows.splice(index..=index, rows);
        true
    }

    fn file_line_tokens(&self, line: u32) -> Option<Vec<Token>> {
        self.file_rows.as_ref()?.iter().find_map(|row| match row {
            PresentedRow::Expanded {
                line: number,
                tokens,
            } if *number == line => Some(tokens.clone()),
            _ => None,
        })
    }

    /// The full-diff row behind an expanded reviewed row.
    pub(crate) fn reviewed_row(&self, hunk: usize, row: usize) -> &DiffRow {
        &self.hunks.reviewed[hunk].rows[row]
    }

    /// The number of changed lines a folded reviewed hunk stands for.
    pub(crate) fn reviewed_changes(&self, hunk: usize) -> (usize, usize) {
        let rows = &self.hunks.reviewed[hunk].rows;
        let count = |added: bool| {
            rows.iter()
                .filter(|row| match row {
                    DiffRow::Add { .. } => added,
                    DiffRow::Delete { .. } => !added,
                    _ => false,
                })
                .count()
        };
        (count(true), count(false))
    }

    /// The control shown in the top-right corner of the hunk that starts at `index`.
    pub(crate) fn hunk_badge(&self, index: usize) -> Option<HunkBadge> {
        match self.rows.get(index)? {
            PresentedRow::Diff { source, .. } => {
                let (hunk, open) = self.open_hunk(*source)?;
                // Only folded rows and unchanged lines separate hunks.
                let previous_hunk = self.rows[..index].iter().rev().find_map(|row| match row {
                    PresentedRow::Diff { source, .. } => Some(self.source_hunks[*source]),
                    _ => None,
                });
                (previous_hunk != Some(Some(hunk))).then_some(HunkBadge::Open {
                    since_review: open.since_review,
                })
            }
            PresentedRow::ReviewedHunk { .. } | PresentedRow::ReviewedLine { row: 0, .. } => {
                Some(HunkBadge::Reviewed)
            }
            _ => None,
        }
    }

    /// The mark that toggles the review of the hunk containing `index`.
    pub(crate) fn hunk_mark(&self, index: usize) -> Option<HunkMark> {
        match self.rows.get(index)? {
            PresentedRow::Diff { source, .. } => {
                let (_, open) = self.open_hunk(*source)?;
                Some(HunkMark::Review(open.span.clone()))
            }
            PresentedRow::ReviewedHunk { hunk } | PresentedRow::ReviewedLine { hunk, .. } => {
                Some(HunkMark::Unreview(self.hunks.reviewed[*hunk].span.clone()))
            }
            PresentedRow::Gap { .. } | PresentedRow::Expanded { .. } => None,
        }
    }

    /// The open hunk a diff row belongs to, with its one-based number.
    fn open_hunk(&self, source: usize) -> Option<(usize, &OpenHunk)> {
        let hunk = self.source_hunks[source]?;
        Some((hunk, self.hunks.open.get(hunk.checked_sub(1)?)?))
    }

    /// How many changed lines are reviewed, once some are.
    pub(crate) fn line_count(&self) -> Option<LineCount> {
        self.hunks.count()
    }
}

/// The parts of an unchanged section outside one-based `folded` lines.
fn split_gap(
    start: u32,
    mut lines: Vec<Vec<Token>>,
    folded: Range<u32>,
) -> impl Iterator<Item = PresentedRow> {
    let gap_end = start.saturating_add(u32::try_from(lines.len()).unwrap_or(u32::MAX));
    let offset = |line: u32| usize::try_from(line.clamp(start, gap_end) - start).unwrap_or(0);
    let after = lines.split_off(offset(folded.end));
    lines.truncate(offset(folded.start));
    let after_start = folded.end.clamp(start, gap_end);
    [
        (!lines.is_empty()).then_some(PresentedRow::Gap { start, lines }),
        (!after.is_empty()).then_some(PresentedRow::Gap {
            start: after_start,
            lines: after,
        }),
    ]
    .into_iter()
    .flatten()
}

fn plain_tokens(row: &DiffRow) -> Vec<Token> {
    let text = match row {
        DiffRow::Context { text, .. }
        | DiffRow::Delete { text, .. }
        | DiffRow::Add { text, .. } => text.get(1..).unwrap_or_default(),
        _ => "",
    };
    vec![Token {
        text: text.to_owned(),
        color: Color::Reset,
    }]
}

#[cfg(test)]
#[path = "hunks.tests.rs"]
mod tests;
