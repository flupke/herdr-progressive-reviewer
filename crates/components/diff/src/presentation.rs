//! Visible rows derived from a parsed unified diff.

use std::cell::OnceCell;
use std::ops::RangeInclusive;
use std::sync::Arc;
pub(super) use text_search::Position as SearchMatch;

use review_repository::diff::DiffRow;
use review_repository::excerpt::{DiffExcerpt, ExcerptError};
use ui_events::{DisplayedDiffRow, DisplayedDiffViewport, PresentationLocation};

use syntax_highlighting::{HighlightedDiff, HighlightedFile, HighlightedRow, Token};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PresentedRow {
    Diff { source: usize, tokens: Vec<Token> },
    Gap { start: u32, lines: Vec<Vec<Token>> },
    Expanded { line: u32, tokens: Vec<Token> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SearchDirection {
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
pub(super) struct DiffPresentation {
    source: Vec<DiffRow>,
    source_hunks: Vec<Option<usize>>,
    pub(super) rows: Vec<PresentedRow>,
    whole_file: Option<WholeFile>,
    file_rows: Option<Vec<PresentedRow>>,
    view: PresentationView,
    search_document: OnceCell<Arc<text_search::Document>>,
}

struct PresentationRows<'a> {
    source: Vec<DiffRow>,
    rows: Vec<PresentedRow>,
    whole_file: Option<WholeFile>,
    after_change_lines: Option<&'a [Vec<Token>]>,
    previous_hunk_end: Option<u32>,
}

impl DiffPresentation {
    pub(super) fn new(highlighted: HighlightedDiff) -> Self {
        let HighlightedDiff {
            rows: highlighted_rows,
            file,
        } = highlighted;
        let after_change_lines = file.as_ref().and_then(HighlightedFile::after_change_lines);
        let whole_file = WholeFile::detect(&highlighted_rows);
        let mut presentation_rows =
            PresentationRows::new(highlighted_rows.len(), whole_file, after_change_lines);
        for highlighted_row in highlighted_rows {
            presentation_rows.push(highlighted_row);
        }
        presentation_rows.finish();
        let PresentationRows { source, rows, .. } = presentation_rows;
        let mut hunk = 0;
        let source_hunks = source
            .iter()
            .map(|row| {
                hunk += usize::from(matches!(row, DiffRow::Hunk { .. }));
                (hunk > 0).then_some(hunk)
            })
            .collect();
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
            source_hunks,
            rows,
            whole_file,
            file_rows,
            view: PresentationView::Diff,
            search_document: OnceCell::new(),
        }
    }

    pub(super) fn apply_highlights(&mut self, highlighted: &HighlightedDiff) {
        for row in self
            .rows
            .iter_mut()
            .chain(self.file_rows.iter_mut().flatten())
        {
            row.apply_highlights(highlighted);
        }
        if let PresentationView::File { diff_rows } = &mut self.view {
            for row in diff_rows {
                row.apply_highlights(highlighted);
            }
        }
    }

    fn gap_lines(lines: Option<&[Vec<Token>]>, start: u32, end: u32) -> Option<Vec<Vec<Token>>> {
        let first = usize::try_from(start.checked_sub(1)?).ok()?;
        let last = usize::try_from(end.checked_sub(1)?).ok()?;
        Some(lines?.get(first..last)?.to_vec())
    }

    pub(super) fn source_row(&self, source: usize) -> &DiffRow {
        &self.source[source]
    }

    fn hunk_number(&self, index: usize) -> Option<usize> {
        let PresentedRow::Diff { source, .. } = self.rows.get(index)? else {
            return None;
        };
        self.source_hunks[*source]
    }

    pub(super) fn guide_viewport(&self, path: String, file_index: usize) -> DisplayedDiffViewport {
        let rows = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let PresentedRow::Diff { source, .. } = row else {
                    return DisplayedDiffRow::default();
                };
                let hunk = self.hunk_number(index);
                match self.source_row(*source) {
                    DiffRow::Context {
                        old_line, new_line, ..
                    } => DisplayedDiffRow {
                        hunk,
                        old_line: Some(*old_line),
                        new_line: Some(*new_line),
                        changed: false,
                    },
                    DiffRow::Delete { old_line, .. } => DisplayedDiffRow {
                        hunk,
                        old_line: Some(*old_line),
                        new_line: None,
                        changed: true,
                    },
                    DiffRow::Add { new_line, .. } => DisplayedDiffRow {
                        hunk,
                        old_line: None,
                        new_line: Some(*new_line),
                        changed: true,
                    },
                    DiffRow::FileHeader { .. }
                    | DiffRow::Meta { .. }
                    | DiffRow::Hunk { .. }
                    | DiffRow::Notice { .. } => DisplayedDiffRow {
                        hunk,
                        ..DisplayedDiffRow::default()
                    },
                }
            })
            .collect();
        DisplayedDiffViewport {
            path,
            file_index,
            rows,
            can_show_file: self.can_show_file(),
        }
    }

    pub(super) fn has_notice(&self) -> bool {
        self.source
            .iter()
            .any(|row| matches!(row, DiffRow::Notice { .. }))
    }

    pub(super) fn is_empty(&self) -> bool {
        self.source.is_empty()
    }

    pub(super) fn shows_whole_file(&self) -> bool {
        self.whole_file.is_some() || self.is_file_view()
    }

    pub(super) fn can_show_file(&self) -> bool {
        self.file_rows.is_some() || self.is_file_view()
    }

    pub(super) fn is_file_view(&self) -> bool {
        matches!(self.view, PresentationView::File { .. })
    }

    pub(super) fn show_file(&mut self) -> bool {
        if self.is_file_view() {
            return false;
        }
        let Some(rows) = self.file_rows.take() else {
            return false;
        };
        self.search_document.take();
        self.view = PresentationView::File {
            diff_rows: std::mem::replace(&mut self.rows, rows),
        };
        true
    }

    pub(super) fn show_diff(&mut self) -> bool {
        let PresentationView::File { diff_rows } = std::mem::take(&mut self.view) else {
            return false;
        };
        self.search_document.take();
        self.file_rows = Some(std::mem::replace(&mut self.rows, diff_rows));
        true
    }

    pub(super) fn len(&self) -> usize {
        self.rows.len()
    }

    pub(super) fn modified_hunk_rows(&self) -> Vec<usize> {
        let mut previous_hunk = None;
        let mut rows = Vec::new();
        for (index, row) in self.rows.iter().enumerate() {
            let PresentedRow::Diff { source, .. } = row else {
                continue;
            };
            if matches!(
                self.source_row(*source),
                DiffRow::Add { .. } | DiffRow::Delete { .. }
            ) {
                let hunk = self.hunk_number(index);
                if hunk != previous_hunk {
                    rows.push(index);
                    previous_hunk = hunk;
                }
            }
        }
        rows
    }

    pub(super) fn is_selectable(&self, index: usize) -> bool {
        self.rows.get(index).is_some_and(|row| match row {
            PresentedRow::Diff { source, .. } => matches!(
                self.source_row(*source),
                DiffRow::Context { .. } | DiffRow::Delete { .. } | DiffRow::Add { .. }
            ),
            PresentedRow::Gap { .. } | PresentedRow::Expanded { .. } => false,
        })
    }

    pub(super) fn source_position(&self, index: usize) -> Option<(u32, String)> {
        let line = match self.rows.get(index)? {
            PresentedRow::Diff { source, .. } => match self.source_row(*source) {
                DiffRow::Context { new_line, .. } | DiffRow::Add { new_line, .. } => {
                    new_line.checked_sub(1)?
                }
                DiffRow::Delete { .. }
                | DiffRow::Notice { .. }
                | DiffRow::FileHeader { .. }
                | DiffRow::Meta { .. }
                | DiffRow::Hunk { .. } => return None,
            },
            PresentedRow::Expanded { line, .. } if self.whole_file != Some(WholeFile::Deleted) => {
                line.checked_sub(1)?
            }
            PresentedRow::Gap { .. } | PresentedRow::Expanded { .. } => return None,
        };
        Some((line, self.source_text(index)?))
    }

    pub(super) fn source_text(&self, index: usize) -> Option<String> {
        let tokens = match self.rows.get(index)? {
            PresentedRow::Diff { source, tokens }
                if matches!(
                    self.source_row(*source),
                    DiffRow::Context { .. } | DiffRow::Delete { .. } | DiffRow::Add { .. }
                ) =>
            {
                tokens
            }
            PresentedRow::Expanded { tokens, .. } => tokens,
            PresentedRow::Diff { .. } | PresentedRow::Gap { .. } => return None,
        };
        Some(tokens.iter().map(|token| token.text.as_str()).collect())
    }

    pub(super) fn presentation_location(&self, index: usize) -> Option<PresentationLocation> {
        match self.rows.get(index)? {
            PresentedRow::Diff { source, .. } => Some(match self.source_row(*source) {
                DiffRow::Context {
                    old_line, new_line, ..
                } => PresentationLocation::Context {
                    old_line: old_line.saturating_sub(1),
                    new_line: new_line.saturating_sub(1),
                },
                DiffRow::Add { new_line, .. } => {
                    PresentationLocation::NewLine(new_line.saturating_sub(1))
                }
                DiffRow::Delete { old_line, .. } => {
                    PresentationLocation::OldLine(old_line.saturating_sub(1))
                }
                DiffRow::Notice { .. }
                | DiffRow::FileHeader { .. }
                | DiffRow::Meta { .. }
                | DiffRow::Hunk { .. } => PresentationLocation::SourceRow(*source),
            }),
            PresentedRow::Expanded { line, .. } => {
                Some(if self.whole_file == Some(WholeFile::Deleted) {
                    PresentationLocation::OldLine(line.saturating_sub(1))
                } else {
                    PresentationLocation::NewLine(line.saturating_sub(1))
                })
            }
            PresentedRow::Gap { start, .. } => Some(PresentationLocation::GapStart(*start)),
        }
    }

    pub(super) fn reveal_presentation_location(
        &mut self,
        location: PresentationLocation,
    ) -> Option<usize> {
        match location {
            PresentationLocation::Context { old_line, new_line } => {
                let _ = self.show_diff();
                let old_display_line = old_line.saturating_add(1);
                let new_display_line = new_line.saturating_add(1);
                self.context_row(|row| row.0 == old_display_line)
                    .or_else(|| self.context_row(|row| row.1 == new_display_line))
            }
            PresentationLocation::NewLine(line) => self.reveal_line(line),
            PresentationLocation::OldLine(line) => {
                let _ = self.show_diff();
                let display_line = line.saturating_add(1);
                self.rows.iter().position(|row| match row {
                    PresentedRow::Diff { source, .. } => matches!(
                        self.source_row(*source),
                        DiffRow::Delete { old_line, .. } if *old_line == display_line
                    ),
                    PresentedRow::Gap { .. } | PresentedRow::Expanded { .. } => false,
                })
            }
            PresentationLocation::SourceRow(target_source) => {
                let _ = self.show_diff();
                self.rows.iter().position(|row| {
                    matches!(row, PresentedRow::Diff { source, .. } if *source == target_source)
                })
            }
            PresentationLocation::GapStart(target_start) => {
                let _ = self.show_diff();
                self.rows.iter().position(|row| match row {
                    PresentedRow::Gap { start, .. } => *start == target_start,
                    PresentedRow::Expanded { line, .. } => *line == target_start,
                    PresentedRow::Diff { .. } => false,
                })
            }
        }
    }

    fn context_row(&self, matches: impl Fn((u32, u32)) -> bool) -> Option<usize> {
        self.rows.iter().position(|row| match row {
            PresentedRow::Diff { source, .. } => match self.source_row(*source) {
                DiffRow::Context {
                    old_line, new_line, ..
                } => matches((*old_line, *new_line)),
                _ => false,
            },
            PresentedRow::Gap { .. } | PresentedRow::Expanded { .. } => false,
        })
    }

    pub(super) fn reveal_line(&mut self, line: u32) -> Option<usize> {
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

    pub(super) fn reveal_diff_line(&mut self, line: u32) -> Option<usize> {
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

    pub(super) fn search_document(&self) -> Arc<text_search::Document> {
        Arc::clone(self.search_document.get_or_init(|| {
            Arc::new(text_search::Document::from_rows(self.rows.iter().map(
                |row| {
                    match row {
                        PresentedRow::Diff { tokens, .. }
                        | PresentedRow::Expanded { tokens, .. } => tokens
                            .iter()
                            .map(|token| token.text.as_str())
                            .collect::<String>(),
                        PresentedRow::Gap { .. } => String::new(),
                    }
                },
            )))
        }))
    }

    pub(super) fn expand(&mut self, index: usize) -> bool {
        let Some(PresentedRow::Gap { start, lines }) = self.rows.get(index).cloned() else {
            return false;
        };
        self.search_document.take();
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

    pub(super) fn expand_all(&mut self) -> bool {
        let mut changed = false;
        let mut index = 0;
        while index < self.rows.len() {
            changed |= self.expand(index);
            index += 1;
        }
        changed
    }

    pub(super) fn contract_all(&mut self) -> bool {
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
        self.search_document.take();
        self.rows = rows;
        true
    }

    pub(super) fn line_number_width(&self) -> usize {
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

    pub(super) fn excerpt(
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

impl PresentedRow {
    fn apply_highlights(&mut self, highlighted: &HighlightedDiff) {
        if let Self::Diff { source, tokens } = self {
            if let Some(row) = highlighted.rows.get(*source) {
                tokens.clone_from(&row.tokens);
            }
            return;
        }
        let Some(file) = &highlighted.file else {
            return;
        };
        let (HighlightedFile::AfterChange(colors) | HighlightedFile::BeforeChange(colors)) = file;
        match self {
            Self::Expanded { line, tokens } => {
                if let Some(color) = colors.get(line.saturating_sub(1) as usize) {
                    tokens.clone_from(color);
                }
            }
            Self::Gap { start, lines } => {
                for (tokens, color) in lines
                    .iter_mut()
                    .zip(colors.iter().skip(start.saturating_sub(1) as usize))
                {
                    tokens.clone_from(color);
                }
            }
            Self::Diff { .. } => unreachable!("diff rows were handled above"),
        }
    }
}

impl WholeFile {
    fn detect(rows: &[HighlightedRow]) -> Option<Self> {
        rows.iter().find_map(|row| match &row.diff {
            DiffRow::Meta { text } if text.starts_with("new file mode ") => Some(Self::Added),
            DiffRow::Meta { text } if text.starts_with("deleted file mode ") => Some(Self::Deleted),
            _ => None,
        })
    }

    fn includes(self, row: &DiffRow) -> bool {
        match self {
            Self::Added => matches!(row, DiffRow::Add { .. }),
            Self::Deleted => matches!(row, DiffRow::Delete { .. }),
        }
    }
}

impl<'a> PresentationRows<'a> {
    fn new(
        capacity: usize,
        whole_file: Option<WholeFile>,
        after_change_lines: Option<&'a [Vec<Token>]>,
    ) -> Self {
        Self {
            source: Vec::with_capacity(capacity),
            rows: Vec::with_capacity(capacity),
            whole_file,
            after_change_lines,
            previous_hunk_end: None,
        }
    }

    fn push(&mut self, highlighted: HighlightedRow) {
        let visible = self.whole_file.map_or_else(
            || {
                matches!(
                    highlighted.diff,
                    DiffRow::Context { .. }
                        | DiffRow::Delete { .. }
                        | DiffRow::Add { .. }
                        | DiffRow::Notice { .. }
                )
            },
            |whole_file| whole_file.includes(&highlighted.diff),
        );
        self.push_gap_before(&highlighted.diff);
        self.source.push(highlighted.diff);
        if visible {
            self.rows.push(PresentedRow::Diff {
                source: self.source.len() - 1,
                tokens: highlighted.tokens,
            });
        }
    }

    fn push_gap_before(&mut self, row: &DiffRow) {
        let DiffRow::Hunk {
            new_start,
            new_count,
            ..
        } = row
        else {
            return;
        };
        let start = self.previous_hunk_end.unwrap_or(1);
        if *new_start > start
            && let Some(lines) =
                DiffPresentation::gap_lines(self.after_change_lines, start, *new_start)
        {
            self.rows.push(PresentedRow::Gap { start, lines });
        }
        self.previous_hunk_end = Some(new_start.saturating_add(*new_count));
    }

    fn finish(&mut self) {
        let Some(start) = self.previous_hunk_end else {
            return;
        };
        let Some(end) = self
            .after_change_lines
            .and_then(|lines| u32::try_from(lines.len()).ok())
            .map(|last| last.saturating_add(1))
        else {
            return;
        };
        if end > start
            && let Some(lines) = DiffPresentation::gap_lines(self.after_change_lines, start, end)
        {
            self.rows.push(PresentedRow::Gap { start, lines });
        }
    }
}
