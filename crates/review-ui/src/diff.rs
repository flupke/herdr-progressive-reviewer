use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use review_lsp::SourceLocation;
use review_repository::diff::{DiffRow, NoticeKind};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::{DiffControl, Focus, ReviewApp, ReviewFile, Selection, text_display_width};
use crate::highlight::Token;
use crate::presentation::{PresentedRow, matching_ranges};
use crate::render::{pane_block, shorten};
use review_state::ReviewStatus;

mod guide;

use guide::GuideBorderCell;
pub(super) use guide::GuideTargetPosition;

pub(super) const TAB_DISPLAY_WIDTH: usize = 4;

pub(super) struct DiffView<'a>(pub(super) &'a ReviewApp);

#[derive(Clone, Copy)]
struct CodeRenderContext<'a> {
    tokens: &'a [Token],
    number_width: usize,
    cursor: Option<usize>,
    source_line: Option<u32>,
    source_location: Option<&'a SourceLocation>,
}

pub(super) struct DiffViewport {
    rows: Vec<WrappedDiffRow>,
}

struct WrappedDiffRow {
    line: Line<'static>,
    guide_border_cells: Vec<GuideBorderCell>,
    source_row: usize,
    source_display_offset: usize,
    is_source_row: bool,
}

impl DiffViewport {
    pub(super) fn scroll(&self, file: &ReviewFile) -> usize {
        file.scroll.min(self.rows.len().saturating_sub(1))
    }

    pub(super) fn scroll_with_cursor_visible(&self, file: &ReviewFile, height: usize) -> usize {
        let last = self.rows.len().saturating_sub(height);
        let mut scroll = file.scroll.min(last);
        let cursor = self.cursor_visual_row(file);
        if cursor < scroll {
            scroll = cursor;
        } else if cursor >= scroll.saturating_add(height) {
            scroll = cursor + 1 - height;
        }
        scroll
    }

    pub(super) fn source_row_at(&self, visual_row: usize) -> Option<usize> {
        self.rows.get(visual_row).map(|row| row.source_row)
    }

    pub(super) fn source_column_at(
        &self,
        visual_row: usize,
        pane_column: usize,
        number_width: usize,
    ) -> Option<usize> {
        let row = self.rows.get(visual_row)?;
        Some(
            row.source_display_offset
                .saturating_add(pane_column.saturating_sub(number_width + 3)),
        )
    }

    pub(super) fn source_position_after_visual_delta(
        &self,
        file: &ReviewFile,
        delta: isize,
    ) -> Option<(usize, usize)> {
        let visual_row = self
            .cursor_visual_row(file)
            .saturating_add_signed(delta)
            .min(self.rows.len().saturating_sub(1));
        let row = self.rows.get(visual_row)?;
        Some((row.source_row, row.source_display_offset))
    }

    pub(super) fn len(&self) -> usize {
        self.rows.len()
    }

    fn cursor_visual_row(&self, file: &ReviewFile) -> usize {
        let source_display_column = file
            .diff
            .source_position(file.cursor)
            .map_or(0, |(_, line)| source_display_width(&line, file.column));
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.is_source_row
                    && row.source_row == file.cursor
                    && row.source_display_offset <= source_display_column
            })
            .map(|(index, _)| index)
            .next_back()
            .or_else(|| {
                self.rows
                    .iter()
                    .position(|row| row.source_row == file.cursor)
            })
            .unwrap_or(0)
    }
}

impl Widget for DiffView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let focused = self.0.focus == Focus::Diff;
        let file = self.0.displayed();
        let inner = self.render_pane(area, buffer, focused, file);
        let Some(file) = file else {
            return;
        };
        if self.render_empty_review(file, inner, buffer) {
            return;
        }
        let viewport = self.viewport(file, inner.width, focused);
        let scroll = viewport.scroll(file);
        let visible_rows = viewport
            .rows
            .iter()
            .skip(scroll)
            .take(usize::from(inner.height))
            .collect::<Vec<_>>();
        let lines = visible_rows
            .iter()
            .map(|row| row.line.clone())
            .collect::<Vec<_>>();
        Paragraph::new(lines).render(inner, buffer);
        for (row_offset, row) in visible_rows.into_iter().enumerate() {
            for border_cell in &row.guide_border_cells {
                let Ok(column) = u16::try_from(border_cell.column) else {
                    continue;
                };
                let Some(cell) = buffer.cell_mut((
                    inner.x.saturating_add(column),
                    inner
                        .y
                        .saturating_add(u16::try_from(row_offset).unwrap_or(u16::MAX)),
                )) else {
                    continue;
                };
                cell.set_char(border_cell.symbol)
                    .set_style(self.guide_style(border_cell.status));
            }
        }
    }
}

impl DiffView<'_> {
    fn render_pane(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        focused: bool,
        file: Option<&ReviewFile>,
    ) -> Rect {
        let title = file.map_or_else(
            || "Diff".to_owned(),
            |file| {
                let kind = if file.diff.is_file_view() {
                    "File"
                } else {
                    "Diff"
                };
                format!("{kind} · {}", file.display_path)
            },
        );
        let controls = DiffControl::title(file);
        let show_controls = DiffControl::visible(area.width, file);
        let title = if show_controls {
            shorten(
                &title,
                usize::from(area.width).saturating_sub(
                    controls.width() + 5 + if focused { " (focus)".len() } else { 0 },
                ),
            )
        } else {
            title
        };
        let mut block = pane_block(self.0, &title, focused);
        if show_controls {
            block = block.title(
                Line::styled(controls, Style::default().fg(self.0.palette.focus))
                    .alignment(Alignment::Right),
            );
        }
        let inner = block.inner(area);
        block.render(area, buffer);
        inner
    }

    fn render_empty_review(&self, file: &ReviewFile, area: Rect, buffer: &mut Buffer) -> bool {
        if file.status == ReviewStatus::Reviewed
            && self.0.search.is_none()
            && file.source_location.is_none()
            && !file.diff.is_file_view()
        {
            let center = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
            Paragraph::new("No changes")
                .style(Style::default().fg(self.0.palette.dim))
                .alignment(Alignment::Center)
                .render(center, buffer);
            return true;
        }
        false
    }

    pub(super) fn viewport(&self, file: &ReviewFile, width: u16, focused: bool) -> DiffViewport {
        let selection = self.0.selection.map(Selection::range);
        let line_number_width = file.diff.line_number_width();
        let show_markers = !file.diff.shows_whole_file();
        let guide_rows = self.guide_rows(file);
        let rows = file
            .diff
            .rows
            .iter()
            .enumerate()
            .flat_map(|(index, presented)| {
                let mut wrapped = Vec::new();
                let guide_row = &guide_rows[index];
                for guide in guide_row.starts.iter().copied() {
                    guide.append(self, &mut wrapped, width, line_number_width);
                }
                let source_line = file.diff.source_position(index).map(|(line, _)| line);
                let context = |tokens| CodeRenderContext {
                    tokens,
                    number_width: line_number_width,
                    cursor: (focused && index == file.cursor).then_some(file.column),
                    source_line,
                    source_location: file.source_location.as_ref(),
                };
                let (line, mut style) = match presented {
                    PresentedRow::Diff { source, tokens } => {
                        let row = file.diff.source_row(*source);
                        (
                            self.diff_line(row, context(tokens), show_markers),
                            self.row_style(row, show_markers),
                        )
                    }
                    PresentedRow::Gap { lines, .. } => (
                        Self::gap_line(lines.len(), line_number_width, usize::from(width)),
                        Style::default()
                            .fg(self.0.palette.text)
                            .bg(self.0.palette.selection),
                    ),
                    PresentedRow::Expanded { line, tokens } => (
                        self.code_line(Some(*line), None, context(tokens)),
                        Style::default().fg(self.0.palette.text),
                    ),
                };
                if selection
                    .as_ref()
                    .is_some_and(|selection| selection.contains(&index))
                {
                    style = style.bg(self.0.palette.selection);
                }
                if focused && index == file.cursor {
                    style = style.bg(self.0.palette.cursor);
                }
                let styled_line = line.style(style);
                let enclosed = guide_row.enclosing_status.is_some();
                wrapped.extend(
                    wrap_line(
                        &styled_line,
                        if enclosed {
                            width.saturating_sub(1)
                        } else {
                            width
                        },
                        line_number_width + 3,
                    )
                    .into_iter()
                    .map(move |(mut line, source_display_offset)| {
                        let guide_border_cells =
                            guide_row.enclosing_status.map_or_else(Vec::new, |status| {
                                Self::reserve_guide_edges(
                                    &mut line,
                                    width,
                                    line_number_width,
                                    status,
                                )
                            });
                        WrappedDiffRow {
                            line,
                            guide_border_cells,
                            source_row: index,
                            source_display_offset,
                            is_source_row: true,
                        }
                    }),
                );
                for guide in guide_row.ends.iter().copied() {
                    guide.append(&mut wrapped, width, line_number_width);
                }
                wrapped
            })
            .collect();
        DiffViewport { rows }
    }

    fn diff_line(
        &self,
        row: &DiffRow,
        context: CodeRenderContext<'_>,
        show_markers: bool,
    ) -> Line<'static> {
        let (line, bar) = match row {
            DiffRow::Add { new_line, .. } => (
                Some(*new_line),
                show_markers.then_some(self.0.palette.insertion),
            ),
            DiffRow::Delete { old_line, .. } => (
                Some(*old_line),
                show_markers.then_some(self.0.palette.deletion),
            ),
            DiffRow::Context { new_line, .. } => (Some(*new_line), None),
            DiffRow::Notice { text, .. } => return Line::raw(text.clone()),
            DiffRow::FileHeader { .. } | DiffRow::Meta { .. } | DiffRow::Hunk { .. } => {
                return Line::default();
            }
        };
        self.code_line(line, bar, context)
    }

    fn code_line(
        &self,
        line: Option<u32>,
        bar: Option<Color>,
        context: CodeRenderContext<'_>,
    ) -> Line<'static> {
        let mut spans = vec![bar.map_or_else(
            || Span::raw("  "),
            |color| {
                Span::styled(
                    "▌ ",
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )
            },
        )];
        spans.push(Span::styled(
            line.map_or_else(
                || " ".repeat(context.number_width + 1),
                |line| format!("{line:>width$} ", width = context.number_width),
            ),
            Style::default().fg(self.0.palette.dim),
        ));
        spans.extend(self.code_spans(
            context.tokens,
            context.cursor,
            context.source_line,
            context.source_location,
        ));
        Line::from(spans)
    }

    fn code_spans(
        &self,
        tokens: &[Token],
        cursor_column: Option<usize>,
        source_line: Option<u32>,
        source_location: Option<&SourceLocation>,
    ) -> Vec<Span<'static>> {
        let query = self
            .0
            .search
            .as_ref()
            .map_or("", |search| search.query.as_str());
        let text = tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect::<String>();
        let source_selection = source_line
            .zip(source_location)
            .and_then(|(line, location)| location.range_in_line(line, text.len()));
        let matches = matching_ranges(&text, query);
        let tab_display = " ".repeat(TAB_DISPLAY_WIDTH);
        let mut spans = Vec::new();
        let mut token_start = 0;
        for token in tokens {
            let token_end = token_start + token.text.len();
            let boundaries = token_boundaries(
                token_start..token_end,
                &matches,
                source_selection.as_ref(),
                cursor_column,
                &text,
            );
            for pair in boundaries.windows(2) {
                let start = pair[0];
                let end = pair[1];
                let style = code_span_style(
                    token.color,
                    start..end,
                    &matches,
                    source_selection.as_ref(),
                    cursor_column,
                );
                spans.push(Span::styled(
                    token.text[start - token_start..end - token_start].replace('\t', &tab_display),
                    style,
                ));
            }
            token_start = token_end;
        }
        if cursor_column == Some(text.len()) {
            spans.push(Span::styled(
                " ",
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            ));
        }
        spans
    }

    fn gap_line(count: usize, number_width: usize, width: usize) -> Line<'static> {
        let mut text = format!("  {:>number_width$} {count} unmodified lines", "…");
        text.push_str(&" ".repeat(width.saturating_sub(text.chars().count())));
        Line::raw(text)
    }

    fn row_style(&self, row: &DiffRow, show_markers: bool) -> Style {
        if !show_markers {
            return Style::default().fg(self.0.palette.text);
        }
        match row {
            DiffRow::Add { .. } => Style::default()
                .fg(self.0.palette.insertion)
                .bg(self.0.palette.insertion_bg),
            DiffRow::Delete { .. } => Style::default()
                .fg(self.0.palette.deletion)
                .bg(self.0.palette.deletion_bg),
            DiffRow::Notice {
                kind: NoticeKind::Binary,
                ..
            } => Style::default().fg(self.0.palette.focus),
            DiffRow::Notice {
                kind: NoticeKind::Conflict | NoticeKind::Unsupported,
                ..
            } => Style::default().fg(self.0.palette.warning),
            DiffRow::Meta { .. } | DiffRow::FileHeader { .. } | DiffRow::Hunk { .. } => {
                Style::default().fg(self.0.palette.dim)
            }
            DiffRow::Context { .. } => Style::default().fg(self.0.palette.text),
        }
    }
}

fn token_boundaries(
    token: Range<usize>,
    matches: &[Range<usize>],
    source_selection: Option<&Range<usize>>,
    cursor_column: Option<usize>,
    text: &str,
) -> Vec<usize> {
    let mut boundaries = vec![token.start, token.end];
    for range in matches
        .iter()
        .filter(|range| range.start < token.end && range.end > token.start)
    {
        boundaries.push(range.start.max(token.start));
        boundaries.push(range.end.min(token.end));
    }
    if let Some(range) =
        source_selection.filter(|range| range.start < token.end && range.end > token.start)
    {
        boundaries.push(range.start.max(token.start));
        boundaries.push(range.end.min(token.end));
    }
    if let Some(column) = cursor_column.filter(|column| {
        *column >= token.start && *column < token.end && text.is_char_boundary(*column)
    }) {
        boundaries.push(column);
        boundaries.push(column + text[column..].chars().next().map_or(0, char::len_utf8));
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
}

fn code_span_style(
    color: Color,
    span: Range<usize>,
    matches: &[Range<usize>],
    source_selection: Option<&Range<usize>>,
    cursor_column: Option<usize>,
) -> Style {
    let selected = matches
        .iter()
        .any(|range| range.start <= span.start && range.end >= span.end)
        || source_selection.is_some_and(|range| range.start <= span.start && range.end >= span.end);
    let mut style = Style::default().fg(color);
    if selected {
        style = style.add_modifier(Modifier::REVERSED);
    }
    if cursor_column == Some(span.start) {
        style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
    }
    style
}

#[derive(Clone)]
struct WrapGrapheme {
    span: Span<'static>,
    display_width: usize,
    is_whitespace: bool,
}

fn wrap_graphemes(line: &Line<'static>) -> Vec<WrapGrapheme> {
    line.styled_graphemes(Style::default())
        .map(|grapheme| {
            let symbol = grapheme.symbol.to_owned();
            WrapGrapheme {
                display_width: symbol.width(),
                is_whitespace: symbol == " " || symbol == "\t",
                span: Span::styled(symbol, grapheme.style),
            }
        })
        .collect()
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
        let grapheme_source_start = consumed_width.saturating_add(content_width);
        content_width = content_width.saturating_add(grapheme.display_width);
        end += 1;
        if grapheme_source_start >= continuation_indent {
            if grapheme.is_whitespace && saw_non_whitespace {
                last_word_boundary = Some(end);
            } else if !grapheme.is_whitespace {
                saw_non_whitespace = true;
            }
        }
    }
    if end < graphemes.len()
        && let Some(word_boundary) = last_word_boundary
    {
        end = word_boundary;
        content_width = graphemes[start..end]
            .iter()
            .map(|grapheme| grapheme.display_width)
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
    let marker = graphemes
        .first()
        .filter(|grapheme| grapheme.span.content == "▌");
    let mut spans = marker
        .map(|grapheme| vec![grapheme.span.clone()])
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

fn wrap_line(
    line: &Line<'static>,
    width: u16,
    continuation_indent: usize,
) -> Vec<(Line<'static>, usize)> {
    let width = usize::from(width.max(1));
    let graphemes = wrap_graphemes(line);
    if graphemes.is_empty() {
        return vec![(Line::default(), 0)];
    }
    let mut wrapped = Vec::new();
    let mut start = 0;
    let mut consumed_width = 0usize;
    while start < graphemes.len() {
        let first_grapheme_width = graphemes[start].display_width;
        let visible_indent = if wrapped.is_empty() {
            0
        } else {
            continuation_indent.min(width.saturating_sub(first_grapheme_width))
        };
        let (end, content_width) = find_wrap_end(
            &graphemes,
            start,
            width.saturating_sub(visible_indent),
            consumed_width,
            continuation_indent,
        );
        let source_display_offset = consumed_width.saturating_sub(continuation_indent);
        let mut spans = continuation_prefix(&graphemes, start, visible_indent);
        spans.extend(
            graphemes[start..end]
                .iter()
                .map(|grapheme| grapheme.span.clone()),
        );
        wrapped.push((Line::from(spans), source_display_offset));
        consumed_width = consumed_width.saturating_add(content_width);
        start = end;
    }
    wrapped
}

#[cfg(test)]
#[path = "diff.tests.rs"]
mod tests;

fn source_display_width(line: &str, byte_column: usize) -> usize {
    line.grapheme_indices(true)
        .take_while(|(byte, grapheme)| byte.saturating_add(grapheme.len()) <= byte_column)
        .map(|(_, grapheme)| text_display_width(grapheme))
        .sum()
}
use std::ops::Range;
