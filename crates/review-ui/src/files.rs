use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use review_repository::repository::ChangeKind;
use review_state::ReviewStatus;
use unicode_width::UnicodeWidthStr;

use crate::app::{Focus, ReviewApp, ReviewFile};
use crate::file_tree::FileTreeRow;
use crate::render::{pane_block, shorten};

pub(super) struct FilesView<'a>(pub(super) &'a ReviewApp);

struct FileStatistics {
    added: Option<String>,
    removed: Option<String>,
}

impl FileStatistics {
    fn new(file: &ReviewFile) -> Self {
        Self {
            added: (file.lines_added > 0).then(|| format!("+{}", file.lines_added)),
            removed: (file.lines_removed > 0).then(|| format!("-{}", file.lines_removed)),
        }
    }

    fn width(&self) -> usize {
        self.added.as_ref().map_or(0, String::len)
            + self.removed.as_ref().map_or(0, String::len)
            + usize::from(self.added.is_some() && self.removed.is_some())
    }

    fn append(self, spans: &mut Vec<Span<'static>>, insertion: Color, deletion: Color) {
        let has_added = self.added.is_some();
        if let Some(added) = self.added {
            spans.push(Span::styled(added, Style::default().fg(insertion)));
        }
        if let Some(removed) = self.removed {
            if has_added {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(removed, Style::default().fg(deletion)));
        }
    }
}

impl Widget for FilesView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let title = self
            .0
            .locations
            .as_ref()
            .map_or("Files", |list| list.operation.title());
        let block = pane_block(self.0, title, self.0.focus == Focus::Files);
        let inner = block.inner(area);
        block.render(area, buffer);
        if let Some(list) = &self.0.locations {
            let lines = list
                .locations
                .iter()
                .enumerate()
                .skip(list.scroll)
                .take(usize::from(inner.height))
                .map(|(index, location)| {
                    let text = format!(
                        "{}:{}:{}",
                        location.display_path(&self.0.repository_root),
                        location.line.saturating_add(1),
                        location.byte_column.saturating_add(1)
                    );
                    let style = Style::default()
                        .fg(self.0.palette.focus)
                        .add_modifier(Modifier::BOLD)
                        .bg(if index == list.selected {
                            self.0.palette.cursor
                        } else {
                            Color::Reset
                        });
                    Line::styled(shorten(&text, usize::from(inner.width)), style)
                })
                .collect::<Vec<_>>();
            Paragraph::new(lines).render(inner, buffer);
            return;
        }
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
        let name = if file.temporary && file.path != file.display_path {
            &file.display_path
        } else {
            name
        };
        let prefix = format!("{}{} ", "  ".repeat(depth), file.marker());
        let comment_marker = self.comment_marker(file);
        let statistics = FileStatistics::new(file);
        let stats_width = statistics.width();
        let gap = usize::from(stats_width > 0);
        let prefix_width = UnicodeWidthStr::width(prefix.as_str());
        let comment_marker_width = comment_marker.map_or(0, UnicodeWidthStr::width);
        let name = shorten(
            name,
            width.saturating_sub(prefix_width + comment_marker_width + stats_width + gap),
        );
        let padding = width.saturating_sub(
            prefix_width
                + UnicodeWidthStr::width(name.as_str())
                + comment_marker_width
                + stats_width,
        );
        let color = self.file_color(file);
        let mut spans = vec![
            Span::styled(prefix, Style::default().fg(color)),
            Span::styled(name, Style::default().fg(color)),
        ];
        if let Some(comment_marker) = comment_marker {
            spans.push(Span::styled(
                comment_marker,
                Style::default().fg(self.0.palette.guide),
            ));
        }
        spans.push(Span::raw(" ".repeat(padding)));
        statistics.append(
            &mut spans,
            self.0.palette.insertion,
            self.0.palette.deletion,
        );
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

    fn comment_marker(&self, file: &ReviewFile) -> Option<&'static str> {
        (file.status != ReviewStatus::Reviewed
            && self
                .0
                .guide_items
                .iter()
                .any(|item| item.target.path() == file.path))
        .then_some(" 💬")
    }

    fn file_color(&self, file: &ReviewFile) -> Color {
        if file.temporary {
            return self.0.palette.dim;
        }
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
