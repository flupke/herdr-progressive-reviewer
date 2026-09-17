use std::collections::HashSet;
use ui_shortcuts::NavigationShortcut;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileTreeRow {
    Directory {
        depth: usize,
        name: String,
        path: String,
        collapsed: bool,
    },
    File {
        depth: usize,
        name: String,
        file: usize,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileTree {
    pub rows: Vec<FileTreeRow>,
}

impl FileTree {
    pub fn new(files: impl Iterator<Item = (String, String)>, collapsed: &HashSet<String>) -> Self {
        let mut tree = Self::expanded(files);
        tree.compact();
        tree.collapse(collapsed);
        tree
    }

    fn expanded(files: impl Iterator<Item = (String, String)>) -> Self {
        let mut rows = Vec::new();
        let mut previous: Vec<String> = Vec::new();
        for (file, (path, display_path)) in files.enumerate() {
            let parts = path.split('/').collect::<Vec<_>>();
            let Some((name, directories)) = parts.split_last() else {
                continue;
            };
            let common = previous
                .iter()
                .zip(directories)
                .take_while(|(left, right)| left.as_str() == **right)
                .count();
            for (depth, name) in directories.iter().enumerate().skip(common) {
                let path = directories[..=depth].join("/");
                rows.push(FileTreeRow::Directory {
                    depth,
                    name: (*name).to_owned(),
                    path,
                    collapsed: false,
                });
            }
            rows.push(FileTreeRow::File {
                depth: directories.len(),
                name: if display_path == path {
                    (*name).to_owned()
                } else {
                    display_path
                },
                file,
            });
            previous = directories.iter().map(|part| (*part).to_owned()).collect();
        }
        Self { rows }
    }

    fn compact(&mut self) {
        let mut index = 0;
        while index + 1 < self.rows.len() {
            if self.merge_child(index) {
                continue;
            }
            index += 1;
        }
    }

    fn merge_child(&mut self, index: usize) -> bool {
        let FileTreeRow::Directory { depth, name, .. } = &self.rows[index] else {
            return false;
        };
        let FileTreeRow::Directory {
            depth: child_depth,
            name: child_name,
            path,
            ..
        } = &self.rows[index + 1]
        else {
            return false;
        };
        let end = self.rows[index + 2..]
            .iter()
            .position(|row| row.depth() <= *child_depth)
            .map_or(self.rows.len(), |offset| index + 2 + offset);
        if self.rows.get(end).is_some_and(|row| row.depth() > *depth) {
            return false;
        }
        let merged = FileTreeRow::Directory {
            depth: *depth,
            name: format!("{name}/{child_name}"),
            path: path.clone(),
            collapsed: false,
        };
        self.rows[index] = merged;
        self.rows.remove(index + 1);
        for row in &mut self.rows[index + 1..end - 1] {
            row.decrease_depth();
        }
        true
    }

    fn collapse(&mut self, collapsed: &HashSet<String>) {
        let mut hidden_depth = None;
        self.rows.retain_mut(|row| {
            if hidden_depth.is_some_and(|depth| row.depth() > depth) {
                return false;
            }
            hidden_depth = None;
            if let FileTreeRow::Directory {
                depth,
                path,
                collapsed: hidden,
                ..
            } = row
            {
                *hidden = collapsed.contains(path);
                if *hidden {
                    hidden_depth = Some(*depth);
                }
            }
            true
        });
    }

    pub fn row_for_file(&self, file: usize) -> Option<usize> {
        self.rows.iter().position(
            |row| matches!(row, FileTreeRow::File { file: candidate, .. } if *candidate == file),
        )
    }

    pub fn nearest_visible_file(&self, file: usize) -> Option<usize> {
        let mut previous = None;
        for candidate in self.visible_files() {
            if candidate >= file {
                return Some(candidate);
            }
            previous = Some(candidate);
        }
        previous
    }

    pub fn visible_files(&self) -> impl DoubleEndedIterator<Item = usize> + '_ {
        self.rows.iter().filter_map(|row| match row {
            FileTreeRow::File { file, .. } => Some(*file),
            FileTreeRow::Directory { .. } => None,
        })
    }

    pub fn navigate(
        &self,
        selected: usize,
        input: NavigationShortcut,
        page_rows: usize,
    ) -> Option<usize> {
        let visible = self.visible_files().collect::<Vec<_>>();
        let current = visible
            .iter()
            .position(|file| *file == selected)
            .unwrap_or(0);
        let target = match input {
            NavigationShortcut::MoveUp => current.saturating_sub(1),
            NavigationShortcut::MoveDown => current.saturating_add(1),
            NavigationShortcut::GoToFirst => 0,
            NavigationShortcut::GoToLast => visible.len().saturating_sub(1),
            NavigationShortcut::MoveHalfPageUp => current.saturating_sub(page_rows.div_ceil(2)),
            NavigationShortcut::MoveHalfPageDown => current.saturating_add(page_rows.div_ceil(2)),
            _ => current,
        }
        .min(visible.len().saturating_sub(1));
        visible.get(target).copied()
    }
}

impl FileTreeRow {
    fn depth(&self) -> usize {
        match self {
            Self::Directory { depth, .. } | Self::File { depth, .. } => *depth,
        }
    }

    fn decrease_depth(&mut self) {
        match self {
            Self::Directory { depth, .. } | Self::File { depth, .. } => {
                *depth = depth.saturating_sub(1);
            }
        }
    }
}
