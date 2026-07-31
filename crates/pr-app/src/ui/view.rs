//! Ratatui rendering for the review state.

use pr_core::diff::{DiffRow, NoticeKind};
use pr_core::repository::ChangeKind;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
use unicode_width::UnicodeWidthStr;

use super::{
    DIFF_CONTROLS_TITLE, Focus, MIN_DIFF_CONTROLS_WIDTH, PaneLayout, ReviewApp, ReviewFile,
    Selection,
};
use crate::file_tree::FileTreeRow;
use crate::highlight::Token;
use crate::presentation::PresentedRow;
use crate::review::ReviewStatus;

const MIN_WIDTH: u16 = 40;
const MIN_HEIGHT: u16 = 6;

/// A side-effect-free view of [`ReviewApp`].
pub struct ReviewView<'a>(pub(super) &'a ReviewApp);

impl Widget for ReviewView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            Paragraph::new("Terminal is too small\nMinimum: 40x6\nq quit").render(area, buffer);
            return;
        }

        let layout = PaneLayout::new(area.width, area.height, self.0.file_width);
        let header = Rect::new(area.x, area.y, area.width, 1);
        let body = Rect::new(area.x, area.y + 1, area.width, layout.body_height());
        let footer = Rect::new(
            area.x,
            area.bottom() - layout.footer_height,
            area.width,
            layout.footer_height,
        );
        self.render_header(header, buffer);
        if layout.is_wide() {
            let file_width = layout.file_width;
            self.render_files(Rect::new(body.x, body.y, file_width, body.height), buffer);
            self.render_diff(
                Rect::new(
                    body.x + file_width,
                    body.y,
                    body.width - file_width,
                    body.height,
                ),
                buffer,
            );
        } else {
            match self.0.focus {
                Focus::Files => self.render_files(body, buffer),
                Focus::Diff => self.render_diff(body, buffer),
            }
        }
        self.render_footer(footer, buffer);
        if self.0.show_commit_message {
            self.render_commit_message(area, buffer);
        }
    }
}

impl ReviewView<'_> {
    fn file_color(&self, file: &ReviewFile) -> Color {
        match file.change {
            ChangeKind::Added => self.0.palette.insertion,
            ChangeKind::Deleted => self.0.palette.deletion,
            ChangeKind::Modified => self.0.palette.focus,
            ChangeKind::Renamed | ChangeKind::TypeChanged | ChangeKind::Conflict => {
                self.0.palette.warning
            }
        }
    }

    fn render_header(&self, area: Rect, buffer: &mut Buffer) {
        let reviewed = self
            .0
            .files
            .iter()
            .filter(|file| file.status == ReviewStatus::Reviewed)
            .count();
        Paragraph::new(format!(" {}", self.0.commit_title()))
            .style(Style::default().fg(self.0.palette.text))
            .render(area, buffer);
        Paragraph::new(format!("{reviewed}/{} reviewed ", self.0.files.len()))
            .alignment(Alignment::Right)
            .style(Style::default().fg(self.0.palette.text))
            .render(area, buffer);
    }

    fn render_commit_message(&self, area: Rect, buffer: &mut Buffer) {
        let width = area.width.saturating_mul(4) / 5;
        let height = area.height.saturating_mul(4) / 5;
        let popup = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        Clear.render(popup, buffer);
        Paragraph::new(if self.0.description.is_empty() {
            "(no description set)"
        } else {
            &self.0.description
        })
        .block(self.block("Commit message", true))
        .wrap(Wrap { trim: false })
        .render(popup, buffer);
    }

    fn render_files(&self, area: Rect, buffer: &mut Buffer) {
        let block = self.block("Files", self.0.focus == Focus::Files);
        let inner = block.inner(area);
        block.render(area, buffer);
        let width = usize::from(inner.width);
        let lines = self
            .0
            .file_tree
            .rows
            .iter()
            .skip(self.0.file_scroll)
            .take(usize::from(inner.height))
            .map(|row| match row {
                FileTreeRow::Directory { depth, name } => Line::styled(
                    format!("{}▾ {name}/", "  ".repeat(*depth)),
                    Style::default()
                        .fg(self.0.palette.dim)
                        .add_modifier(Modifier::BOLD),
                ),
                FileTreeRow::File { depth, name, file } => {
                    self.file_line(*depth, name, *file, width)
                }
            })
            .collect::<Vec<_>>();
        Paragraph::new(lines).render(inner, buffer);
    }

    fn file_line(&self, depth: usize, name: &str, index: usize, width: usize) -> Line<'static> {
        let file = &self.0.files[index];
        let prefix = format!("{}{} ", "  ".repeat(depth), file.marker());
        let added = (file.lines_added > 0).then(|| format!("+{}", file.lines_added));
        let removed = (file.lines_removed > 0).then(|| format!("-{}", file.lines_removed));
        let has_added = added.is_some();
        let stats_width = added.as_ref().map_or(0, String::len)
            + removed.as_ref().map_or(0, String::len)
            + usize::from(added.is_some() && removed.is_some());
        let gap = usize::from(stats_width > 0);
        let name = Self::shorten(
            name,
            width.saturating_sub(prefix.chars().count() + stats_width + gap),
        );
        let padding =
            width.saturating_sub(prefix.chars().count() + name.chars().count() + stats_width);
        let color = self.file_color(file);
        let mut spans = vec![
            Span::styled(prefix, Style::default().fg(color)),
            Span::styled(name, Style::default().fg(color)),
            Span::raw(" ".repeat(padding)),
        ];
        if let Some(added) = added {
            spans.push(Span::styled(
                added,
                Style::default().fg(self.0.palette.insertion),
            ));
        }
        if let Some(removed) = removed {
            if has_added {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                removed,
                Style::default().fg(self.0.palette.deletion),
            ));
        }
        let style = if index == self.0.selected_file {
            Style::default().bg(self.0.palette.cursor)
        } else {
            Style::default()
        };
        Line::from(spans).style(style)
    }

    fn render_diff(&self, area: Rect, buffer: &mut Buffer) {
        let focused = self.0.focus == Focus::Diff;
        let title = self.0.selected().map_or_else(
            || "Diff".to_owned(),
            |file| format!("Diff · {}", file.display_path),
        );
        let show_controls = area.width >= MIN_DIFF_CONTROLS_WIDTH;
        let title = if show_controls {
            Self::shorten(
                &title,
                usize::from(area.width).saturating_sub(
                    DIFF_CONTROLS_TITLE.width() + 5 + if focused { " (focus)".len() } else { 0 },
                ),
            )
        } else {
            title
        };
        let mut block = self.block(&title, focused);
        if show_controls {
            block = block.title(
                Line::styled(
                    DIFF_CONTROLS_TITLE,
                    Style::default().fg(self.0.palette.focus),
                )
                .alignment(Alignment::Right),
            );
        }
        let inner = block.inner(area);
        block.render(area, buffer);
        let Some(file) = self.0.selected() else {
            return;
        };
        if file.status == ReviewStatus::Reviewed {
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

    fn render_footer(&self, area: Rect, buffer: &mut Buffer) {
        let controls = "Tab focus · j/k move · l expand · v select · c message · Enter insert · Space review · q quit";
        let status = match self.0.review_in_flight {
            Some(true) => "Marking reviewed…",
            Some(false) => "Removing review mark…",
            None => self.0.notice.as_deref().unwrap_or(""),
        };
        let lines = if area.height == 1 {
            vec![Line::from(vec![Span::raw(if status.is_empty() {
                controls
            } else {
                status
            })])]
        } else {
            vec![Line::raw(controls), Line::raw(status)]
        };
        Paragraph::new(lines)
            .style(Style::default().fg(self.0.palette.text))
            .render(area, buffer);
    }

    fn block(&self, title: &str, focused: bool) -> Block<'_> {
        let style = if focused {
            Style::default().fg(self.0.palette.focus)
        } else {
            Style::default().fg(self.0.palette.dim)
        };
        let suffix = if focused { " (focus)" } else { "" };
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title}{suffix} "))
            .border_style(style)
    }

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
        spans.extend(
            tokens
                .iter()
                .map(|token| Span::styled(token.text.clone(), Style::default().fg(token.color))),
        );
        Line::from(spans)
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

    fn shorten(value: &str, width: usize) -> String {
        let length = value.chars().count();
        if length <= width {
            return value.to_owned();
        }
        if width <= 3 {
            return ".".repeat(width);
        }
        let left = (width - 1) / 2;
        let right = width - left - 1;
        format!(
            "{}…{}",
            value.chars().take(left).collect::<String>(),
            value.chars().skip(length - right).collect::<String>()
        )
    }
}
