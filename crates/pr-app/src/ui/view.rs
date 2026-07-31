//! Ratatui rendering for the review state.

use pr_core::diff::{DiffRow, NoticeKind};
use pr_core::repository::ChangeKind;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use super::{Focus, PaneLayout, ReviewApp, ReviewFile, Selection};
use crate::file_tree::FileTreeRow;
use crate::highlight::Token;
use crate::presentation::RowDisplay;
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
        let change = self.0.change_id.chars().take(8).collect::<String>();
        Paragraph::new(format!(
            " Progressive review · change {change} · {reviewed}/{} reviewed",
            self.0.files.len()
        ))
        .style(Style::default().fg(self.0.palette.text))
        .render(area, buffer);
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
        let block = self.block(&title, focused);
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
        let lines = file
            .diff
            .rows
            .iter()
            .enumerate()
            .skip(file.scroll)
            .take(usize::from(inner.height))
            .map(|(index, presented)| {
                let row = file.diff.row(presented);
                let mut style = self.row_style(row, presented.display);
                if selection
                    .as_ref()
                    .is_some_and(|selection| selection.contains(&index))
                {
                    style = style.bg(self.0.palette.selection);
                }
                if focused && index == file.cursor {
                    style = style.bg(self.0.palette.cursor);
                }
                self.row_line(row, &file.path, &presented.tokens, presented.display)
                    .style(style)
            })
            .collect::<Vec<_>>();
        Paragraph::new(lines).render(inner, buffer);
    }

    fn render_footer(&self, area: Rect, buffer: &mut Buffer) {
        let controls = "Tab focus · j/k move · v select · Enter insert · Space review · q quit";
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

    fn row_text(row: &DiffRow, path: &str) -> String {
        match row {
            DiffRow::FileHeader { .. } => format!("diff --git {path}"),
            DiffRow::Meta { text }
            | DiffRow::Context { text, .. }
            | DiffRow::Delete { text, .. }
            | DiffRow::Add { text, .. }
            | DiffRow::Notice { text, .. } => text.clone(),
            DiffRow::Hunk {
                old_start,
                old_count,
                new_start,
                new_count,
            } => format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@"),
        }
    }

    fn row_line(
        &self,
        row: &DiffRow,
        path: &str,
        tokens: &[Token],
        display: RowDisplay,
    ) -> Line<'static> {
        if display == RowDisplay::FileContent {
            return Line::from(
                tokens
                    .iter()
                    .map(|token| Span::styled(token.text.clone(), Style::default().fg(token.color)))
                    .collect::<Vec<_>>(),
            );
        }
        let marker = match row {
            DiffRow::Add { .. } => Some(("+", self.0.palette.insertion)),
            DiffRow::Delete { .. } => Some(("-", self.0.palette.deletion)),
            DiffRow::Context { .. } => Some((" ", self.0.palette.dim)),
            _ => None,
        };
        let Some((marker, color)) = marker else {
            return Line::raw(Self::row_text(row, path));
        };
        let mut spans = vec![Span::styled(marker, Style::default().fg(color))];
        spans.extend(
            tokens
                .iter()
                .map(|token| Span::styled(token.text.clone(), Style::default().fg(token.color))),
        );
        Line::from(spans)
    }

    fn row_style(&self, row: &DiffRow, display: RowDisplay) -> Style {
        if display == RowDisplay::FileContent {
            return Style::default().fg(self.0.palette.text);
        }
        match row {
            DiffRow::Add { .. } => Style::default()
                .fg(self.0.palette.insertion)
                .bg(self.0.palette.insertion_bg),
            DiffRow::Delete { .. } => Style::default()
                .fg(self.0.palette.deletion)
                .bg(self.0.palette.deletion_bg),
            DiffRow::Hunk { .. }
            | DiffRow::Notice {
                kind: NoticeKind::Binary,
                ..
            } => Style::default().fg(self.0.palette.focus),
            DiffRow::Notice {
                kind: NoticeKind::Conflict | NoticeKind::Unsupported,
                ..
            } => Style::default().fg(self.0.palette.warning),
            DiffRow::Meta { .. } | DiffRow::FileHeader { .. } => {
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
