use pr_core::repository::ChangeKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::{pane_block, shorten};
use crate::file_tree::FileTreeRow;
use crate::ui::{Focus, ReviewApp, ReviewFile};

pub(super) struct FilesView<'a>(pub(super) &'a ReviewApp);

impl Widget for FilesView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let block = pane_block(self.0, "Files", self.0.focus == Focus::Files);
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
                FileTreeRow::Directory {
                    depth,
                    name,
                    collapsed,
                    ..
                } => Line::styled(
                    format!(
                        "{}{} {name}/",
                        "  ".repeat(*depth),
                        if *collapsed { '▸' } else { '▾' }
                    ),
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
}

impl FilesView<'_> {
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
        let name = shorten(
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
        let mut style = if index == self.0.selected_file {
            Style::default().bg(self.0.palette.cursor)
        } else {
            Style::default()
        };
        if self.0.file_matches_search(index) {
            style = style.add_modifier(Modifier::BOLD);
        }
        Line::from(spans).style(style)
    }

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
}
