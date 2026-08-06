//! Collapsible directory rows for the changed-file list.

use std::collections::HashSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileTreeRow {
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
pub(crate) struct FileTree {
    pub(crate) rows: Vec<FileTreeRow>,
}

impl FileTree {
    pub(crate) fn new<'a>(
        files: impl Iterator<Item = (&'a str, &'a str)>,
        collapsed: &HashSet<String>,
    ) -> Self {
        let mut rows = Vec::new();
        let mut previous = Vec::new();
        for (file, (path, label)) in files.enumerate() {
            let parts = path.split('/').collect::<Vec<_>>();
            let Some((name, directories)) = parts.split_last() else {
                continue;
            };
            let common = previous
                .iter()
                .zip(directories)
                .take_while(|(left, right)| left == right)
                .count();
            let mut hidden = (0..common)
                .map(|depth| directories[..=depth].join("/"))
                .any(|path| collapsed.contains(&path));
            for (depth, name) in directories.iter().enumerate().skip(common) {
                if hidden {
                    break;
                }
                let path = directories[..=depth].join("/");
                let is_collapsed = collapsed.contains(&path);
                rows.push(FileTreeRow::Directory {
                    depth,
                    name: (*name).to_owned(),
                    path,
                    collapsed: is_collapsed,
                });
                hidden = is_collapsed;
            }
            if !hidden {
                rows.push(FileTreeRow::File {
                    depth: directories.len(),
                    name: if label == path { name } else { label }.to_string(),
                    file,
                });
            }
            previous = directories.to_vec();
        }
        Self { rows }
    }

    pub(crate) fn row_for_file(&self, file_index: usize) -> Option<usize> {
        self.rows.iter().position(
            |row| matches!(row, FileTreeRow::File { file: candidate, .. } if *candidate == file_index),
        )
    }

    pub(crate) fn file_at(&self, row_index: usize) -> Option<usize> {
        match self.rows.get(row_index)? {
            FileTreeRow::File { file, .. } => Some(*file),
            FileTreeRow::Directory { .. } => None,
        }
    }

    pub(crate) fn directory_at(&self, row_index: usize) -> Option<(&str, usize)> {
        match self.rows.get(row_index)? {
            FileTreeRow::Directory { path, depth, .. } => Some((path, *depth)),
            FileTreeRow::File { .. } => None,
        }
    }

    pub(crate) fn visible_file_at(&self, visible_position: usize) -> Option<usize> {
        self.visible_files().nth(visible_position)
    }

    pub(crate) fn visible_file_position(&self, file_index: usize) -> Option<usize> {
        self.visible_files()
            .position(|candidate| candidate == file_index)
    }

    pub(crate) fn visible_file_count(&self) -> usize {
        self.visible_files().count()
    }

    pub(crate) fn nearest_visible_file(&self, file_index: usize) -> Option<usize> {
        let mut previous = None;
        for candidate in self.visible_files() {
            if candidate >= file_index {
                return Some(candidate);
            }
            previous = Some(candidate);
        }
        previous
    }

    pub(crate) fn visible_files(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows.iter().filter_map(|row| match row {
            FileTreeRow::File { file, .. } => Some(*file),
            FileTreeRow::Directory { .. } => None,
        })
    }
}

#[cfg(test)]
#[path = "file_tree.tests.rs"]
mod tests;
