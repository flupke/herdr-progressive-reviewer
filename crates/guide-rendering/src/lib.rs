//! Review-guide layout and rendering primitives.

mod frame;

pub use frame::{DiffFrame, FrameRule};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use review_guide::{GuideItem, GuideItemStatus, GuideTarget};
use ui_events::{DisplayedDiffRow, DisplayedDiffViewport, GuideCounter};
use unicode_width::UnicodeWidthStr;

struct PositionedGuide<'a> {
    item: &'a GuideItem,
    first_row: usize,
    last_row: usize,
    counter: GuideCounter,
}

#[derive(Clone, Copy)]
struct GuideStart<'a> {
    text: &'a str,
    target_row: usize,
    is_file_target: bool,
    status: GuideItemStatus,
    counter: GuideCounter,
}

#[derive(Clone, Copy)]
struct GuideEnd {
    target_row: usize,
    status: GuideItemStatus,
}

#[derive(Default)]
struct GuideLayoutRow<'a> {
    starts: Vec<GuideStart<'a>>,
    enclosing_status: Option<GuideItemStatus>,
    ends: Vec<GuideEnd>,
}

/// One guide border cell that is drawn over a diff row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuideBorderCell {
    /// Zero-based column inside the diff content area.
    pub column: usize,
    /// Border character.
    pub symbol: char,
    /// Visible guide style.
    pub style: Style,
}

/// One complete visual row produced by a guide box.
pub struct GuideRenderedRow {
    /// Styled row content.
    pub line: Line<'static>,
    /// Border cells that must preserve the background below them.
    pub border_cells: Vec<GuideBorderCell>,
    /// Presented diff row that owns this guide row.
    pub source_row: usize,
}

/// One guide layer row positioned inside the visible diff content area.
pub struct GuideOverlayRow {
    /// Zero-based row inside the diff content area.
    pub row: u16,
    /// Optional guide text or rule for this row.
    pub line: Option<Line<'static>>,
    /// Border cells drawn after the diff row to preserve its background.
    pub border_cells: Vec<GuideBorderCell>,
}

/// The visible guide layer produced while the diff viewport is composed.
pub struct GuideOverlay {
    area: Rect,
    rows: Vec<GuideOverlayRow>,
}

impl GuideOverlay {
    /// Create an empty guide layer.
    pub fn empty() -> Self {
        Self {
            area: Rect::default(),
            rows: Vec::new(),
        }
    }

    /// Create a guide layer for one visible diff content area.
    pub fn new(area: Rect, rows: Vec<GuideOverlayRow>) -> Self {
        Self { area, rows }
    }

    /// Draw the guide layer after the diff surface.
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

/// Guide boxes positioned against one displayed diff document.
pub struct GuideLayout<'a> {
    rows: Vec<GuideLayoutRow<'a>>,
    guide_color: Color,
}

impl<'a> GuideLayout<'a> {
    pub fn new(
        items: &'a [GuideItem],
        counters: &[Option<GuideCounter>],
        row_count: usize,
        guide_color: Color,
        target_rows: impl Fn(&GuideTarget) -> Option<(usize, usize)>,
    ) -> Self {
        let positioned = items
            .iter()
            .enumerate()
            .filter_map(|(item_index, item)| {
                let (first_row, last_row) = target_rows(&item.target)?;
                let counter = counters.get(item_index).copied().flatten()?;
                (first_row <= last_row && last_row < row_count).then_some(PositionedGuide {
                    item,
                    first_row,
                    last_row,
                    counter,
                })
            })
            .collect::<Vec<_>>();
        let mut rows = std::iter::repeat_with(GuideLayoutRow::default)
            .take(row_count)
            .collect::<Vec<_>>();
        for guide in positioned {
            rows[guide.first_row].starts.push(guide.start());
            for row in &mut rows[guide.first_row..=guide.last_row] {
                row.enclosing_status.get_or_insert(guide.item.status);
            }
            if let Some(end) = guide.end() {
                rows[guide.last_row].ends.push(end);
            }
        }
        Self { rows, guide_color }
    }

    /// Return guide rows that must appear before one diff row.
    pub fn rows_before(
        &self,
        source_row: usize,
        width: u16,
        line_number_width: usize,
    ) -> Vec<GuideRenderedRow> {
        self.rows
            .get(source_row)
            .into_iter()
            .flat_map(|row| row.starts.iter().copied())
            .flat_map(|start| self.render_start(start, width, line_number_width))
            .collect()
    }

    /// Return guide rows that must appear after one diff row.
    pub fn rows_after(
        &self,
        source_row: usize,
        width: u16,
        line_number_width: usize,
    ) -> Vec<GuideRenderedRow> {
        self.rows
            .get(source_row)
            .into_iter()
            .flat_map(|row| row.ends.iter().copied())
            .map(|end| {
                self.render_rule(
                    FrameRule::Bottom,
                    width,
                    line_number_width,
                    end.target_row,
                    end.status,
                )
            })
            .collect()
    }

    /// Return the guide state that encloses one diff row.
    pub fn enclosing_status(&self, source_row: usize) -> Option<GuideItemStatus> {
        self.rows.get(source_row)?.enclosing_status
    }

    /// Frame geometry and styling for a guide's diff rows.
    pub fn frame(
        &self,
        width: u16,
        line_number_width: usize,
        status: GuideItemStatus,
    ) -> DiffFrame {
        DiffFrame::new(width, line_number_width, self.style(status))
    }

    /// Return the visible style for one guide state.
    fn style(&self, status: GuideItemStatus) -> Style {
        let style = Style::default().fg(self.guide_color);
        if status == GuideItemStatus::Stale {
            style.add_modifier(Modifier::DIM)
        } else {
            style
        }
    }

    fn render_start(
        &self,
        start: GuideStart<'a>,
        width: u16,
        line_number_width: usize,
    ) -> Vec<GuideRenderedRow> {
        let counter = format!(" {}/{} ", start.counter.number, start.counter.total);
        let mut rows = vec![self.render_rule(
            FrameRule::Top(Some(&counter)),
            width,
            line_number_width,
            start.target_row,
            start.status,
        )];
        rows.extend(self.render_text(
            start.text,
            width,
            line_number_width,
            start.target_row,
            start.status,
        ));
        rows.push(self.render_rule(
            if start.is_file_target {
                FrameRule::Bottom
            } else {
                FrameRule::Middle
            },
            width,
            line_number_width,
            start.target_row,
            start.status,
        ));
        rows
    }

    fn render_rule(
        &self,
        kind: FrameRule<'_>,
        width: u16,
        line_number_width: usize,
        source_row: usize,
        status: GuideItemStatus,
    ) -> GuideRenderedRow {
        self.frame(width, line_number_width, status)
            .rule(kind, source_row)
    }

    fn render_text(
        &self,
        text: &str,
        width: u16,
        line_number_width: usize,
        source_row: usize,
        status: GuideItemStatus,
    ) -> Vec<GuideRenderedRow> {
        let style = self.style(status);
        self.frame(width, line_number_width, status)
            .text(text, style, source_row)
    }
}

/// Find the tight changed-row range for one guide target in one viewport.
pub fn target_rows(
    viewport: &DisplayedDiffViewport,
    target: &GuideTarget,
) -> Option<(usize, usize)> {
    match target {
        GuideTarget::Hunks {
            path,
            first_hunk,
            last_hunk,
        } if path == &viewport.path => matching_rows(&viewport.rows, |row| {
            row.changed
                && row
                    .hunk
                    .is_some_and(|hunk| *first_hunk <= hunk && hunk <= *last_hunk)
        }),
        GuideTarget::Lines { path, old, new } if path == &viewport.path => {
            matching_rows(&viewport.rows, |row| {
                line_matches(row.old_line, old.as_ref()) || line_matches(row.new_line, new.as_ref())
            })
        }
        GuideTarget::File { path } if path == &viewport.path => Some((0, 0)),
        GuideTarget::Hunks { .. } | GuideTarget::Lines { .. } | GuideTarget::File { .. } => None,
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

fn line_matches(line: Option<u32>, range: Option<&review_guide::GuideLineRange>) -> bool {
    line.zip(range)
        .is_some_and(|(line, range)| (range.first_line..=range.last_line).contains(&line))
}

impl<'a> PositionedGuide<'a> {
    fn start(&self) -> GuideStart<'a> {
        GuideStart {
            text: &self.item.text,
            target_row: self.first_row,
            is_file_target: matches!(self.item.target, GuideTarget::File { .. }),
            status: self.item.status,
            counter: self.counter,
        }
    }

    fn end(&self) -> Option<GuideEnd> {
        (!matches!(self.item.target, GuideTarget::File { .. })).then_some(GuideEnd {
            target_row: self.first_row,
            status: self.item.status,
        })
    }
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
