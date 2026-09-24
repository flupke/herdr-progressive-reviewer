//! Shared changed-file list layout and tree interaction for Files and Explore.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
};
use ui_shortcuts::NavigationShortcut;
use ui_theme::Palette;

use super::{FileTree, FileTreeRow, SelectionPane, shorten};

pub struct FileList<'a> {
    pub tree: &'a FileTree,
    pub selected: usize,
    pub scroll: usize,
    pub page_rows: usize,
}

impl FileList<'_> {
    pub fn render(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        palette: Palette,
        focused: bool,
        title: &str,
        file_row: impl Fn(usize, &str, usize, usize) -> Line<'static>,
    ) {
        let suffix = if focused { " (focus)" } else { "" };
        SelectionPane::new(area, self.scroll).render(
            buffer,
            format!(" {title}{suffix} "),
            Style::default().fg(if focused { palette.focus } else { palette.dim }),
            &self.tree.rows,
            |_, row, width| match row {
                FileTreeRow::Directory {
                    depth,
                    name,
                    collapsed,
                    ..
                } => {
                    let label = format!(
                        "{}{} {name}/",
                        "  ".repeat(*depth),
                        if *collapsed { '▸' } else { '▾' }
                    );
                    Line::styled(
                        shorten(&label, usize::from(width)),
                        Style::default()
                            .fg(palette.dim)
                            .add_modifier(Modifier::BOLD),
                    )
                }
                FileTreeRow::File { depth, name, file } => {
                    file_row(*depth, name, *file, usize::from(width))
                }
            },
        );
    }

    pub fn navigate(&self, input: NavigationShortcut) -> usize {
        self.tree
            .navigate(self.selected, input, self.page_rows)
            .unwrap_or(self.selected)
    }

    pub fn row(&self, row: usize) -> Option<&FileTreeRow> {
        self.tree.rows.get(self.scroll.saturating_add(row))
    }

    pub fn visible_scroll(&self, selected: usize) -> usize {
        let Some(row) = self.tree.row_for_file(selected) else {
            return 0;
        };
        if self.tree.visible_files().next() == Some(selected) {
            return row.saturating_add(1).saturating_sub(self.page_rows);
        }
        if row < self.scroll {
            row
        } else if row >= self.scroll.saturating_add(self.page_rows) {
            row.saturating_add(1).saturating_sub(self.page_rows)
        } else {
            self.scroll
        }
    }

    pub fn scroll_by(&self, delta: isize) -> usize {
        self.scroll
            .saturating_add_signed(delta)
            .min(self.tree.rows.len().saturating_sub(self.page_rows))
    }
}
