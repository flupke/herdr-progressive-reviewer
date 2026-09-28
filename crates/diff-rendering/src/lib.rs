//! Diff frame and source-target rendering primitives.

mod frame;

pub use frame::{DiffFrame, FrameRule};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use review_source::DiffTarget;
use ui_events::{DisplayedDiffRow, DisplayedDiffViewport};
use unicode_width::UnicodeWidthStr;

/// One frame border cell drawn over a diff row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameBorderCell {
    /// Zero-based column inside the diff content area.
    pub column: usize,
    /// Border character.
    pub symbol: char,
    /// Visible frame style.
    pub style: Style,
}

/// One complete visual row produced by a frame.
pub struct FramedRow {
    /// Styled row content.
    pub line: Line<'static>,
    /// Border cells that must preserve the background below them.
    pub border_cells: Vec<FrameBorderCell>,
    /// Presented diff row that owns this frame row.
    pub source_row: usize,
}

/// One frame layer row positioned inside the visible diff content area.
pub struct FrameOverlayRow {
    /// Zero-based row inside the diff content area.
    pub row: u16,
    /// Optional frame text or rule for this row.
    pub line: Option<Line<'static>>,
    /// Border cells drawn after the diff row to preserve its background.
    pub border_cells: Vec<FrameBorderCell>,
}

/// The visible frame layer produced while the diff viewport is composed.
pub struct FrameOverlay {
    area: Rect,
    rows: Vec<FrameOverlayRow>,
}

impl FrameOverlay {
    /// Create an empty frame layer.
    pub fn empty() -> Self {
        Self {
            area: Rect::default(),
            rows: Vec::new(),
        }
    }

    /// Create a frame layer for one visible diff content area.
    pub fn new(area: Rect, rows: Vec<FrameOverlayRow>) -> Self {
        Self { area, rows }
    }

    /// Draw the frame layer after the diff surface.
    pub fn render(&self, buffer: &mut Buffer) {
        for row in &self.rows {
            let y = self.area.y.saturating_add(row.row);
            if let Some(line) = &row.line {
                Paragraph::new(line.clone())
                    .render(Rect::new(self.area.x, y, self.area.width, 1), buffer);
            }
            for border_cell in &row.border_cells {
                let Ok(column) = u16::try_from(border_cell.column) else {
                    continue;
                };
                let Some(cell) = buffer.cell_mut((self.area.x.saturating_add(column), y)) else {
                    continue;
                };
                cell.set_char(border_cell.symbol)
                    .set_style(border_cell.style);
            }
        }
    }
}

/// Find the tight changed-row range for one source target in one viewport.
pub fn target_rows(
    viewport: &DisplayedDiffViewport,
    target: &DiffTarget,
) -> Option<(usize, usize)> {
    match target {
        DiffTarget::Lines { path, old, new } if path == &viewport.path => {
            matching_rows(&viewport.rows, |row| {
                line_matches(row.old_line, old.as_ref()) || line_matches(row.new_line, new.as_ref())
            })
        }
        DiffTarget::File { path } if path == &viewport.path => Some((0, 0)),
        DiffTarget::Lines { .. } | DiffTarget::File { .. } => None,
    }
}

fn matching_rows(
    rows: &[DisplayedDiffRow],
    matches: impl Fn(&DisplayedDiffRow) -> bool,
) -> Option<(usize, usize)> {
    let mut matching = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| matches(row).then_some(index));
    let first = matching.next()?;
    Some((first, matching.next_back().unwrap_or(first)))
}

fn line_matches(line: Option<u32>, range: Option<&review_source::SourceLineRange>) -> bool {
    line.zip(range)
        .is_some_and(|(line, range)| (range.first_line..=range.last_line).contains(&line))
}

#[derive(Clone)]
struct WrapGrapheme {
    span: Span<'static>,
    display_width: usize,
    is_whitespace: bool,
}

fn wrap_line(line: &Line<'static>, width: u16, continuation_indent: usize) -> Vec<Line<'static>> {
    let width = usize::from(width.max(1));
    let graphemes = line
        .styled_graphemes(Style::default())
        .map(|grapheme| {
            let symbol = grapheme.symbol.to_owned();
            WrapGrapheme {
                display_width: symbol.width(),
                is_whitespace: symbol == " " || symbol == "\t",
                span: Span::styled(symbol, grapheme.style),
            }
        })
        .collect::<Vec<_>>();
    if graphemes.is_empty() {
        return vec![Line::default()];
    }
    let mut wrapped = Vec::new();
    let mut start = 0;
    let mut consumed_width = 0usize;
    while start < graphemes.len() {
        let visible_indent = if wrapped.is_empty() {
            0
        } else {
            continuation_indent.min(width.saturating_sub(graphemes[start].display_width))
        };
        let (end, content_width) = find_wrap_end(
            &graphemes,
            start,
            width.saturating_sub(visible_indent),
            consumed_width,
            continuation_indent,
        );
        let mut spans = continuation_prefix(&graphemes, start, visible_indent);
        spans.extend(graphemes[start..end].iter().map(|part| part.span.clone()));
        wrapped.push(Line::from(spans));
        consumed_width = consumed_width.saturating_add(content_width);
        start = end;
    }
    wrapped
}

fn find_wrap_end(
    graphemes: &[WrapGrapheme],
    start: usize,
    available_width: usize,
    consumed_width: usize,
    continuation_indent: usize,
) -> (usize, usize) {
    let mut end = start;
    let mut content_width = 0usize;
    let mut last_word_boundary = None;
    let mut saw_non_whitespace = false;
    while end < graphemes.len() {
        let grapheme = &graphemes[end];
        if end > start && content_width.saturating_add(grapheme.display_width) > available_width {
            break;
        }
        let source_start = consumed_width.saturating_add(content_width);
        content_width = content_width.saturating_add(grapheme.display_width);
        end += 1;
        if source_start >= continuation_indent {
            if grapheme.is_whitespace && saw_non_whitespace {
                last_word_boundary = Some(end);
            } else if !grapheme.is_whitespace {
                saw_non_whitespace = true;
            }
        }
    }
    if end < graphemes.len()
        && let Some(boundary) = last_word_boundary
    {
        end = boundary;
        content_width = graphemes[start..end]
            .iter()
            .map(|part| part.display_width)
            .sum();
    }
    (end, content_width)
}

fn continuation_prefix(
    graphemes: &[WrapGrapheme],
    start: usize,
    visible_indent: usize,
) -> Vec<Span<'static>> {
    if visible_indent == 0 {
        return Vec::new();
    }
    let mut spans = graphemes
        .first()
        .filter(|part| part.span.content == "▌")
        .map(|part| vec![part.span.clone()])
        .unwrap_or_default();
    let blank_width = visible_indent.saturating_sub(spans.len());
    if blank_width > 0 {
        spans.push(Span::styled(
            " ".repeat(blank_width),
            graphemes[start].span.style,
        ));
    }
    spans
}
