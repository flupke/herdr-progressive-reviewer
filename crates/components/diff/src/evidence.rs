//! Transient yellow frames use native presentation coordinates and wrapping.
use super::{Color, DiffFrame, DiffRenderer, LoadedDocument, Style, WrappedDiffRow};
use guide_rendering::FrameRule;
use review_explore::SourceSide;
use review_guide::GuideLineRange;
use std::ops::Range;

struct EvidenceRows {
    range: Range<usize>,
    total: usize,
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
        let viewport = self.viewport(file, width.max(1), false);
        let mut matched = viewport.rows.iter().enumerate().filter_map(|(index, row)| {
            let line = file.document.diff.evidence_line(row.source_row, side)?;
            range
                .is_some_and(|range| range.first_line <= line && line <= range.last_line)
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

    fn matched_range(
        file: &LoadedDocument,
        row: usize,
        ranges: &[(SourceSide, GuideLineRange)],
    ) -> Option<(usize, u32)> {
        ranges
            .iter()
            .enumerate()
            .find_map(|(index, (side, range))| {
                let line = file.document.diff.evidence_line(row, *side)?;
                (range.first_line <= line && line <= range.last_line).then_some((index, line))
            })
    }

    pub(super) fn evidence_frame(
        &self,
        file: &LoadedDocument,
        row: usize,
        width: u16,
    ) -> Option<DiffFrame> {
        let ranges = self.evidence?.ranges(file);
        Self::matched_range(file, row, &ranges).map(|_| Self::yellow_frame(file, width))
    }

    fn yellow_frame(file: &LoadedDocument, width: u16) -> DiffFrame {
        DiffFrame::new(
            width,
            file.document.diff.line_number_width(),
            Style::default().fg(Color::Yellow),
        )
    }

    pub(super) fn outline(
        &self,
        rows: Vec<WrappedDiffRow>,
        file: &LoadedDocument,
        width: u16,
    ) -> Vec<WrappedDiffRow> {
        let ranges = self
            .evidence
            .map(|evidence| evidence.ranges(file))
            .unwrap_or_default();
        if ranges.is_empty() {
            return rows;
        }
        let matches: Vec<_> = rows
            .iter()
            .map(|row| {
                row.is_source_row
                    .then(|| Self::matched_range(file, row.source_row, &ranges))
                    .flatten()
            })
            .collect();
        let frame = Self::yellow_frame(file, width);
        let mut outlined = Vec::new();
        for (index, mut row) in rows.into_iter().enumerate() {
            let Some((range, line)) = matches[index] else {
                outlined.push(row);
                continue;
            };
            let previous = index.checked_sub(1).and_then(|index| matches[index]);
            let next = matches.get(index + 1).copied().flatten();
            if previous.is_none_or(|(previous, _)| previous != range)
                && line == ranges[range].1.first_line
            {
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
            if next.is_none_or(|(next, _)| next != range) && line == ranges[range].1.last_line {
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
