//! Read-only changed-file tree for a checkpoint-bound Explore preview.
use std::{cell::Cell, collections::HashSet};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget},
};
use ui_shortcuts::{Key, NavigationShortcut};
use ui_theme::Palette;

use super::{FileTree, FileTreeRow, shorten};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewFile {
    pub file: usize,
    pub path: String,
    pub required: u64,
}

/// File selection, scrolling, and drawing shared with the Files tree's model.
pub struct FilePreviewList {
    files: Vec<PreviewFile>,
    tree: FileTree,
    selected: usize,
    scroll: Cell<usize>,
    area: Cell<Rect>,
}

impl FilePreviewList {
    pub fn new(mut files: Vec<PreviewFile>) -> Self {
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let tree = FileTree::new(
            files
                .iter()
                .map(|file| (file.path.clone(), file.path.clone())),
            &HashSet::default(),
        );
        Self {
            files,
            tree,
            selected: 0,
            scroll: Cell::new(0),
            area: Cell::new(Rect::default()),
        }
    }

    pub fn selected(&self) -> Option<&PreviewFile> {
        self.files.get(self.selected)
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn move_key(&mut self, key: Key) -> bool {
        let before = self.selected;
        let input = match key {
            Key::Down | Key::Char('j') => NavigationShortcut::MoveDown,
            Key::Up | Key::Char('k') => NavigationShortcut::MoveUp,
            Key::PageDown | Key::HalfPageDown => NavigationShortcut::MoveHalfPageDown,
            Key::PageUp | Key::HalfPageUp => NavigationShortcut::MoveHalfPageUp,
            Key::First => NavigationShortcut::GoToFirst,
            Key::Last => NavigationShortcut::GoToLast,
            _ => return false,
        };
        self.selected = self
            .tree
            .navigate(
                self.selected,
                input,
                usize::from(self.area.get().height.saturating_sub(2).max(1)),
            )
            .unwrap_or(before);
        self.show_selected();
        before != self.selected
    }

    pub fn select_at(&mut self, column: u16, row: u16) -> bool {
        let area = self.area.get();
        if column <= area.x
            || column >= area.right().saturating_sub(1)
            || row <= area.y
            || row >= area.bottom().saturating_sub(1)
        {
            return false;
        }
        let index = self.scroll.get() + usize::from(row - area.y - 1);
        let Some(file) = self.tree.file_at(index) else {
            return false;
        };
        let changed = file != self.selected;
        self.selected = file;
        self.show_selected();
        changed
    }

    pub fn scroll_by(&self, delta: isize) {
        let visible = usize::from(self.area.get().height.saturating_sub(2).max(1));
        self.scroll.set(
            self.scroll
                .get()
                .saturating_add_signed(delta)
                .min(self.tree.rows.len().saturating_sub(visible)),
        );
    }

    pub fn contains(&self, column: u16, row: u16) -> bool {
        let area = self.area.get();
        column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette, focused: bool) {
        self.area.set(area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Not explored · Files ")
            .border_style(Style::default().fg(if focused { palette.focus } else { palette.dim }));
        let inner = block.inner(area);
        block.render(area, buffer);
        let visible = usize::from(inner.height.max(1));
        let rows = self
            .tree
            .rows
            .iter()
            .skip(self.scroll.get())
            .take(visible)
            .map(|row| match row {
                FileTreeRow::Directory { depth, name, .. } => Line::styled(
                    shorten(
                        &format!("{}{name}/", "  ".repeat(*depth)),
                        usize::from(inner.width),
                    ),
                    Style::default()
                        .fg(palette.dim)
                        .add_modifier(Modifier::BOLD),
                ),
                FileTreeRow::File { depth, name, file } => {
                    let entry = &self.files[*file];
                    let label = format!(
                        "{}{} {name} · {} required",
                        "  ".repeat(*depth),
                        if *file == self.selected { '▸' } else { ' ' },
                        entry.required,
                    );
                    Line::styled(
                        shorten(&label, usize::from(inner.width)),
                        Style::default().fg(if *file == self.selected {
                            palette.focus
                        } else {
                            palette.text
                        }),
                    )
                }
            })
            .collect::<Vec<_>>();
        Paragraph::new(rows).render(inner, buffer);
    }

    fn show_selected(&self) {
        let visible = usize::from(self.area.get().height.saturating_sub(2).max(1));
        if let Some(row) = self.tree.row_for_file(self.selected) {
            let scroll = self.scroll.get();
            if row < scroll {
                self.scroll.set(row);
            } else if row >= scroll + visible {
                self.scroll.set(row + 1 - visible);
            }
        }
    }
}
