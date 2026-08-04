use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use review_repository::diff::{DiffRow, NoticeKind};
use unicode_width::UnicodeWidthStr;

use super::{pane_block, shorten};
use crate::highlight::Token;
use crate::presentation::{PresentedRow, matching_ranges};
use crate::ui::{DiffControl, Focus, ReviewApp, Selection};
use review_state::ReviewStatus;

pub(super) struct DiffView<'a>(pub(super) &'a ReviewApp);

impl Widget for DiffView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let focused = self.0.focus == Focus::Diff;
        let file = self.0.selected();
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
        let Some(file) = file else {
            return;
        };
        if file.status == ReviewStatus::Reviewed
            && self.0.search.is_none()
            && !file.diff.is_file_view()
        {
            let center = Rect::new(inner.x, inner.y + inner.height / 2, inner.width, 1);
            Paragraph::new("No changes")
                .style(Style::default().fg(self.0.palette.dim))
                .alignment(Alignment::Center)
                .render(center, buffer);
            return;
        }
        let selection = self.0.selection.map(Selection::range);
        let line_number_width = file.diff.line_number_width();
        let show_markers = !file.diff.shows_whole_file();
        let lines = file
            .diff
            .rows
            .iter()
            .enumerate()
            .skip(file.scroll)
            .take(usize::from(inner.height))
            .map(|(index, presented)| {
                let (line, mut style) = match presented {
                    PresentedRow::Diff { source, tokens } => {
                        let row = file.diff.source_row(*source);
                        (
                            self.diff_line(row, tokens, line_number_width, show_markers),
                            self.row_style(row, show_markers),
                        )
                    }
                    PresentedRow::Gap { lines, .. } => (
                        Self::gap_line(lines.len(), line_number_width, usize::from(inner.width)),
                        Style::default()
                            .fg(self.0.palette.text)
                            .bg(self.0.palette.selection),
                    ),
                    PresentedRow::Expanded { line, tokens } => (
                        self.code_line(Some(*line), None, tokens, line_number_width),
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
            .collect::<Vec<_>>();
        Paragraph::new(lines).render(inner, buffer);
    }
}

impl DiffView<'_> {
    fn diff_line(
        &self,
        row: &DiffRow,
        tokens: &[Token],
        number_width: usize,
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
        self.code_line(line, bar, tokens, number_width)
    }

    fn code_line(
        &self,
        line: Option<u32>,
        bar: Option<Color>,
        tokens: &[Token],
        number_width: usize,
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
                || " ".repeat(number_width + 1),
                |line| format!("{line:>number_width$} "),
            ),
            Style::default().fg(self.0.palette.dim),
        ));
        spans.extend(self.code_spans(tokens));
        Line::from(spans)
    }

    fn code_spans(&self, tokens: &[Token]) -> Vec<Span<'static>> {
        let query = self
            .0
            .search
            .as_ref()
            .map_or("", |search| search.query.as_str());
        let text = tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect::<String>();
        let matches = matching_ranges(&text, query);
        let mut spans = Vec::new();
        let mut token_start = 0;
        for token in tokens {
            let token_end = token_start + token.text.len();
            let mut cursor = token_start;
            for range in matches
                .iter()
                .filter(|range| range.start < token_end && range.end > token_start)
            {
                let start = range.start.max(token_start);
                let end = range.end.min(token_end);
                if cursor < start {
                    spans.push(Span::styled(
                        token.text[cursor - token_start..start - token_start].to_owned(),
                        Style::default().fg(token.color),
                    ));
                }
                spans.push(Span::styled(
                    token.text[start - token_start..end - token_start].to_owned(),
                    Style::default()
                        .fg(self.0.palette.deletion)
                        .bg(self.0.palette.warning)
                        .add_modifier(Modifier::BOLD),
                ));
                cursor = end;
            }
            if cursor < token_end {
                spans.push(Span::styled(
                    token.text[cursor - token_start..].to_owned(),
                    Style::default().fg(token.color),
                ));
            }
            token_start = token_end;
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
