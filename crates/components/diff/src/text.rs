//! The text a diff viewer drew, for the reviewer to select and copy: the code of each line
//! without its gutter, and the prose of the threads framed under it.
//!
//! A plain drag over the code comments on its lines, so the code's region needs Alt held.
//! A drag means nothing over a thread, so each run of thread rows lies over it in a region
//! of its own, where a plain drag selects the thread's text.

use std::ops::Range;

use diff_rendering::DiffFrame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ui_text_selection::{RowEnd, TextCell, TextRegion, TextRow};
use unicode_segmentation::UnicodeSegmentation;

use super::{DiffPointerViewport, code_column, grapheme_display_width};
use crate::DiffPresentation;

impl DiffPointerViewport {
    /// The text of the rows drawn in `area`, the whole viewer's first, then each run of
    /// thread rows over it: a line of code keeps its tabs, and its rows join where it wrapped.
    pub(crate) fn text_regions(
        &self,
        area: Rect,
        diff: &DiffPresentation,
        buffer: &Buffer,
    ) -> Vec<TextRegion> {
        let mut region = TextRegion::new(area).needing_alt();
        let mut threads = Vec::new();
        let code_column = u16::try_from(code_column(self.line_number_width)).unwrap_or(u16::MAX);
        let frame = DiffFrame::new(area.width, self.line_number_width, Style::default());
        let mut thread_rows: Option<Range<u16>> = None;
        for (index, row) in self.rows.iter().enumerate() {
            let y = area
                .y
                .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
            if row.is_source_row {
                if let Some(rows) = thread_rows.take() {
                    threads.push(thread_text(&mut region, frame, area, rows, buffer));
                }
                let next = self
                    .rows
                    .get(index + 1)
                    .filter(|next| next.is_source_row && next.source_row == row.source_row);
                if let Some(text) = diff.source_text(row.source_row) {
                    let shown = usize::from(area.width.saturating_sub(code_column));
                    let end = next.map_or(row.source_display_offset + shown, |next| {
                        next.source_display_offset
                    });
                    region.push(code_row(
                        &text,
                        row.source_display_offset..end,
                        Position::new(area.x.saturating_add(code_column), y),
                    ));
                }
            } else {
                thread_rows.get_or_insert(y..y).end = y + 1;
            }
        }
        if let Some(rows) = thread_rows {
            threads.push(thread_text(&mut region, frame, area, rows, buffer));
        }
        let mut regions = vec![region];
        regions.append(&mut threads);
        regions
    }
}

/// The region of the thread `rows`, whose prose is also added to the viewer's `region`.
fn thread_text(
    region: &mut TextRegion,
    frame: DiffFrame,
    area: Rect,
    rows: Range<u16>,
    buffer: &Buffer,
) -> TextRegion {
    let prose = framed_prose_area(frame, area, rows);
    region.push_prose(prose, buffer);
    let mut thread = TextRegion::new(prose);
    thread.push_prose(prose, buffer);
    thread
}

/// The graphemes of the line `text` that start in the display `columns` a row shows.
fn code_row(text: &str, columns: Range<usize>, position: Position) -> TextRow {
    let mut cells = Vec::new();
    let mut column = 0;
    for grapheme in text.graphemes(true) {
        if column >= columns.end {
            return TextRow::new(position, cells, RowEnd::Wrap { space: false });
        }
        let width = grapheme_display_width(grapheme);
        if column >= columns.start {
            cells.push(TextCell::new(
                grapheme,
                u16::try_from(width).unwrap_or(u16::MAX),
            ));
        }
        column += width;
    }
    TextRow::new(position, cells, RowEnd::Line)
}

/// Add the prose of `rows` of a pane's `area`, read inside the padding of `frame`.
pub(crate) fn push_framed_prose(
    region: &mut TextRegion,
    frame: DiffFrame,
    area: Rect,
    rows: Range<u16>,
    buffer: &Buffer,
) {
    region.push_prose(framed_prose_area(frame, area, rows), buffer);
}

/// Where `rows` of a pane's `area` hold prose, inside the padding of `frame`.
fn framed_prose_area(frame: DiffFrame, area: Rect, rows: Range<u16>) -> Rect {
    let column = u16::try_from(frame.content_column()).unwrap_or(u16::MAX);
    Rect::new(
        area.x.saturating_add(column),
        rows.start,
        frame.content_width(),
        rows.end - rows.start,
    )
}
