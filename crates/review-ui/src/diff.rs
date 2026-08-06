use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use review_lsp::SourceLocation;
use review_repository::diff::{DiffRow, NoticeKind};
use unicode_width::UnicodeWidthStr;

use crate::app::{DiffControl, Focus, ReviewApp, ReviewFile, Selection};
use crate::highlight::Token;
use crate::presentation::{PresentedRow, matching_ranges};
use crate::render::{pane_block, shorten};
use review_state::ReviewStatus;

pub(super) struct DiffView<'a>(pub(super) &'a ReviewApp);

#[derive(Clone, Copy)]
struct CodeRenderContext<'a> {
    tokens: &'a [Token],
    number_width: usize,
    cursor: Option<usize>,
    source_line: Option<u32>,
    source_location: Option<&'a SourceLocation>,
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
        let lines = self.render_lines(file, inner, focused);
        Paragraph::new(lines)
            .scroll((0, u16::try_from(file.horizontal_scroll).unwrap_or(u16::MAX)))
            .render(inner, buffer);
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

    fn render_lines(&self, file: &ReviewFile, area: Rect, focused: bool) -> Vec<Line<'static>> {
        let selection = self.0.selection.map(Selection::range);
        let line_number_width = file.diff.line_number_width();
        let show_markers = !file.diff.shows_whole_file();
        file.diff
            .rows
            .iter()
            .enumerate()
            .skip(file.scroll)
            .take(usize::from(area.height))
            .map(|(index, presented)| {
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
                        Self::gap_line(lines.len(), line_number_width, usize::from(area.width)),
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
                line.style(style)
            })
            .collect()
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
        let mut spans = Vec::new();
        let mut token_start = 0;
        for token in tokens {
            let token_end = token_start + token.text.len();
            let mut boundaries = vec![token_start, token_end];
            for range in matches
                .iter()
                .filter(|range| range.start < token_end && range.end > token_start)
            {
                boundaries.push(range.start.max(token_start));
                boundaries.push(range.end.min(token_end));
            }
            if let Some(range) = source_selection
                .as_ref()
                .filter(|range| range.start < token_end && range.end > token_start)
            {
                boundaries.push(range.start.max(token_start));
                boundaries.push(range.end.min(token_end));
            }
            if let Some(column) = cursor_column.filter(|column| {
                *column >= token_start && *column < token_end && text.is_char_boundary(*column)
            }) {
                boundaries.push(column);
                boundaries.push(column + text[column..].chars().next().map_or(0, char::len_utf8));
            }
            boundaries.sort_unstable();
            boundaries.dedup();
            for pair in boundaries.windows(2) {
                let start = pair[0];
                let end = pair[1];
                let mut style = Style::default().fg(token.color);
                if matches
                    .iter()
                    .any(|range| range.start <= start && range.end >= end)
                    || source_selection
                        .as_ref()
                        .is_some_and(|range| range.start <= start && range.end >= end)
                {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                if cursor_column == Some(start) {
                    style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
                }
                spans.push(Span::styled(
                    token.text[start - token_start..end - token_start].replace('\t', "    "),
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
