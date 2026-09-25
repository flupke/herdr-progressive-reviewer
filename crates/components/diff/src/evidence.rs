//! Transient yellow frames use native presentation coordinates and wrapping.
use super::{Color, DiffFrame, DiffRenderer, LoadedDocument, Style, WrappedDiffRow};
use crate::PresentedRow;
use crate::explore::ExploreView;
use guide_rendering::FrameRule;
use review_explore::SourceSide;
use review_guide::GuideLineRange;
use review_repository::diff::DiffRow;
use std::collections::BTreeSet;
use std::ops::{Range, RangeInclusive};

struct EvidenceRows {
    range: Range<usize>,
    total: usize,
}

/// Project citations onto displayed rows before wrapping. Opposite-side changes
/// between a citation's endpoints remain inside its outline.
pub(super) struct EvidenceFrames {
    spans: Vec<EvidenceSpan>,
    frame: DiffFrame,
}

struct EvidenceSpan {
    rows: RangeInclusive<usize>,
    starts_here: bool,
    ends_here: bool,
}

/// Keep only required changed rows and the surrounding hunk context in a
/// conclusion preview. Source rows remain untouched for navigation and frames.
pub(super) struct RequiredEvidenceRows {
    visible: Vec<bool>,
}

impl RequiredEvidenceRows {
    pub(super) fn new(file: &LoadedDocument, evidence: Option<&ExploreView>) -> Option<Self> {
        let evidence = evidence.filter(|evidence| evidence.active && evidence.required_only)?;
        let ranges = evidence.ranges(file);
        let rows = &file.document.diff.rows;
        let mut row_hunks = Vec::with_capacity(rows.len());
        let mut required = vec![false; rows.len()];
        let mut required_hunks = BTreeSet::new();
        let mut hunk = 0;
        for (index, presented) in rows.iter().enumerate() {
            if let PresentedRow::Diff { source, .. } = presented {
                match file.document.diff.source_row(*source) {
                    DiffRow::Hunk { .. } => hunk += 1,
                    DiffRow::Add { new_line, .. } => {
                        required[index] = Self::contains(&ranges, SourceSide::New, *new_line);
                    }
                    DiffRow::Delete { old_line, .. } => {
                        required[index] = Self::contains(&ranges, SourceSide::Old, *old_line);
                    }
                    _ => {}
                }
                if required[index] {
                    required_hunks.insert(hunk);
                }
            }
            row_hunks.push(hunk);
        }
        let side = if file.new_path.is_some() {
            SourceSide::New
        } else {
            SourceSide::Old
        };
        let visible = rows
            .iter()
            .enumerate()
            .map(|(index, presented)| match presented {
                PresentedRow::Diff { source, .. } => match file.document.diff.source_row(*source) {
                    DiffRow::FileHeader { .. } | DiffRow::Meta { .. } | DiffRow::Notice { .. } => {
                        true
                    }
                    DiffRow::Hunk { .. } | DiffRow::Context { .. } => {
                        required_hunks.contains(&row_hunks[index])
                    }
                    DiffRow::Add { .. } | DiffRow::Delete { .. } => required[index],
                },
                PresentedRow::Expanded { line, .. } => Self::contains(&ranges, side, *line),
                PresentedRow::Gap { .. } => false,
            })
            .collect();
        Some(Self { visible })
    }

    fn contains(ranges: &[(SourceSide, GuideLineRange)], side: SourceSide, line: u32) -> bool {
        ranges.iter().any(|(candidate, range)| {
            *candidate == side && range.first_line <= line && line <= range.last_line
        })
    }

    pub(super) fn shows(&self, index: usize) -> bool {
        self.visible.get(index).copied().unwrap_or(false)
    }
}

impl EvidenceSpan {
    fn new(file: &LoadedDocument, side: SourceSide, range: &GuideLineRange) -> Option<Self> {
        let mut matching = (0..file.document.diff.len()).filter_map(|row| {
            let line = file.document.diff.evidence_line(row, side)?;
            (range.first_line <= line && line <= range.last_line).then_some((row, line))
        });
        let first = matching.next()?;
        let last = matching.next_back().unwrap_or(first);
        Some(Self {
            rows: first.0..=last.0,
            starts_here: first.1 == range.first_line,
            ends_here: last.1 == range.last_line,
        })
    }

    fn merge(&mut self, next: &Self) {
        if self.rows.start() == next.rows.start() {
            self.starts_here &= next.starts_here;
        }
        match self.rows.end().cmp(next.rows.end()) {
            std::cmp::Ordering::Less => {
                self.rows = *self.rows.start()..=*next.rows.end();
                self.ends_here = next.ends_here;
            }
            std::cmp::Ordering::Equal => self.ends_here &= next.ends_here,
            std::cmp::Ordering::Greater => {}
        }
    }
}

impl EvidenceFrames {
    pub(super) fn new(file: &LoadedDocument, width: u16, evidence: Option<&ExploreView>) -> Self {
        let ranges = evidence
            .map(|evidence| evidence.ranges(file))
            .unwrap_or_default();
        let mut spans: Vec<_> = ranges
            .iter()
            .filter_map(|(side, range)| EvidenceSpan::new(file, *side, range))
            .collect();
        spans.sort_by_key(|span| *span.rows.start());
        let mut merged: Vec<EvidenceSpan> = Vec::new();
        for span in spans {
            if let Some(previous) = merged
                .last_mut()
                .filter(|previous| span.rows.start() <= previous.rows.end())
            {
                previous.merge(&span);
            } else {
                merged.push(span);
            }
        }
        Self {
            spans: merged,
            frame: DiffFrame::new(
                width,
                file.document.diff.line_number_width(),
                Style::default().fg(Color::Yellow),
            ),
        }
    }

    fn span_at(&self, row: usize) -> Option<usize> {
        self.spans.iter().position(|span| span.rows.contains(&row))
    }

    pub(super) fn frame_at(&self, row: usize) -> Option<DiffFrame> {
        self.span_at(row).map(|_| self.frame)
    }

    fn selected_rows(
        &self,
        file: &LoadedDocument,
        side: SourceSide,
        range: Option<&GuideLineRange>,
    ) -> Option<RangeInclusive<usize>> {
        let selected = EvidenceSpan::new(file, side, range?)?;
        let outline = self.spans.iter().find(|span| {
            span.rows.contains(selected.rows.start()) && span.rows.contains(selected.rows.end())
        });
        Some(outline.map_or(selected.rows, |span| span.rows.clone()))
    }
}

impl EvidenceRows {
    fn height(&self) -> u16 {
        // Three context rows on either side plus native borders. The range includes outlines.
        u16::try_from((self.range.len() + 6).min(self.total) + 2).unwrap_or(u16::MAX)
    }

    fn scroll(&self, height: usize) -> usize {
        let before = height.saturating_sub(self.range.len()).min(3);
        self.range
            .start
            .saturating_sub(before)
            .min(self.total.saturating_sub(height))
    }
}

impl DiffRenderer<'_> {
    pub(crate) fn evidence_height(
        &self,
        file: &LoadedDocument,
        width: u16,
        side: SourceSide,
        range: Option<&GuideLineRange>,
    ) -> u16 {
        self.evidence_rows(file, width, side, range).height()
    }

    pub(crate) fn evidence_scroll(
        &self,
        file: &LoadedDocument,
        width: u16,
        height: u16,
        side: SourceSide,
        range: Option<&GuideLineRange>,
    ) -> usize {
        self.evidence_rows(file, width, side, range)
            .scroll(usize::from(height))
    }

    fn evidence_rows(
        &self,
        file: &LoadedDocument,
        width: u16,
        side: SourceSide,
        range: Option<&GuideLineRange>,
    ) -> EvidenceRows {
        let width = width.max(1);
        let frames = EvidenceFrames::new(file, width, self.evidence);
        let selected = frames.selected_rows(file, side, range);
        let viewport = self.viewport_with_frames(file, width, false, &frames);
        let mut matched = viewport.rows.iter().enumerate().filter_map(|(index, row)| {
            selected
                .as_ref()
                .is_some_and(|range| range.contains(&row.source_row))
                .then_some(index)
        });
        let range = matched
            .next()
            .map_or(0..3.min(viewport.rows.len()), |start| {
                start..matched.next_back().unwrap_or(start).saturating_add(1)
            });
        EvidenceRows {
            range,
            total: viewport.rows.len(),
        }
    }
}

impl EvidenceFrames {
    pub(super) fn outline(&self, rows: Vec<WrappedDiffRow>) -> Vec<WrappedDiffRow> {
        if self.spans.is_empty() {
            return rows;
        }
        let matches: Vec<_> = rows
            .iter()
            .map(|row| {
                row.is_source_row
                    .then(|| self.span_at(row.source_row))
                    .flatten()
            })
            .collect();
        let frame = self.frame;
        let mut outlined = Vec::new();
        for (index, mut row) in rows.into_iter().enumerate() {
            let Some(range) = matches[index] else {
                outlined.push(row);
                continue;
            };
            let previous = index.checked_sub(1).and_then(|index| matches[index]);
            let next = matches.get(index + 1).copied().flatten();
            let span = &self.spans[range];
            if previous != Some(range) && row.source_row == *span.rows.start() && span.starts_here {
                outlined.push(Self::evidence_rule(
                    frame,
                    FrameRule::Top(Some(" relevant ")),
                    row.source_row,
                ));
            }
            row.guide_border_cells
                .extend(frame.enclose_line(&mut row.line));
            let source_row = row.source_row;
            outlined.push(row);
            if next != Some(range) && source_row == *span.rows.end() && span.ends_here {
                outlined.push(Self::evidence_rule(frame, FrameRule::Bottom, source_row));
            }
        }
        outlined
    }

    fn evidence_rule(frame: DiffFrame, rule: FrameRule<'_>, row: usize) -> WrappedDiffRow {
        let rendered = frame.rule(rule, row);
        let mut row = WrappedDiffRow::source(rendered.line, rendered.border_cells, row, 0);
        row.is_source_row = false;
        row
    }
}
