//! Read-only changed-file tree for a checkpoint-bound Explore preview.
use std::{cell::Cell, collections::HashSet};

use component_core::InputResolution;
use ratatui::{buffer::Buffer, layout::Rect, style::Style, text::Line};
use ui_shortcuts::{Key, ShortcutCommand, ShortcutMatcher, ShortcutSet};
use ui_theme::Palette;

use super::{FileList, FileTree, FileTreeRow, shorten};

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
    collapsed_directories: HashSet<String>,
    selected: usize,
    scroll: Cell<usize>,
    area: Cell<Rect>,
    keys: ShortcutMatcher,
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
            collapsed_directories: HashSet::new(),
            selected: 0,
            scroll: Cell::new(0),
            area: Cell::new(Rect::default()),
            keys: ShortcutMatcher::new(ShortcutSet::Files),
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
        let InputResolution::Matched(ShortcutCommand::Navigation(input)) =
            self.keys.resolve_key(key)
        else {
            return false;
        };
        self.selected = self.list().navigate(input);
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
        let index = usize::from(row - area.y - 1);
        match self.list().row(index).cloned() {
            Some(FileTreeRow::File { file, .. }) => {
                let changed = file != self.selected;
                self.selected = file;
                self.show_selected();
                changed
            }
            Some(FileTreeRow::Directory { depth, path, .. })
                if column
                    == area.x.saturating_add(
                        1 + u16::try_from(depth.saturating_mul(2)).unwrap_or(u16::MAX),
                    ) =>
            {
                let previous = self.selected;
                if !self.collapsed_directories.remove(&path) {
                    self.collapsed_directories.insert(path);
                }
                self.tree = FileTree::new(
                    self.files
                        .iter()
                        .map(|file| (file.path.clone(), file.path.clone())),
                    &self.collapsed_directories,
                );
                if self.tree.row_for_file(self.selected).is_none() {
                    self.selected = self.tree.nearest_visible_file(self.selected).unwrap_or(0);
                }
                self.show_selected();
                self.selected != previous
            }
            _ => false,
        }
    }

    pub fn scroll_by(&self, delta: isize) {
        self.scroll.set(self.list().scroll_by(delta));
    }

    pub fn contains(&self, column: u16, row: u16) -> bool {
        let area = self.area.get();
        column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette, focused: bool) {
        self.area.set(area);
        self.list().render(
            area,
            buffer,
            palette,
            focused,
            "Not explored · Files",
            |depth, name, file, width| {
                let entry = &self.files[file];
                let label = format!(
                    "{}○ {name} · {} required",
                    "  ".repeat(depth),
                    entry.required
                );
                let style = if file == self.selected {
                    Style::default().fg(palette.focus).bg(palette.cursor)
                } else {
                    Style::default().fg(palette.text)
                };
                Line::styled(shorten(&label, width), style)
            },
        );
    }

    fn show_selected(&self) {
        self.scroll.set(self.list().visible_scroll(self.selected));
    }

    fn list(&self) -> FileList<'_> {
        FileList {
            tree: &self.tree,
            selected: self.selected,
            scroll: self.scroll.get(),
            page_rows: usize::from(self.area.get().height.saturating_sub(2).max(1)),
        }
    }
}
