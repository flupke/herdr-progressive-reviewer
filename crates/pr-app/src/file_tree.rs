//! Expanded directory rows for the changed-file list.

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileTreeRow {
    Directory {
        depth: usize,
        name: String,
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
    pub(crate) fn new<'a>(files: impl Iterator<Item = (&'a str, &'a str)>) -> Self {
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
            rows.extend(
                directories
                    .iter()
                    .enumerate()
                    .skip(common)
                    .map(|(depth, name)| FileTreeRow::Directory {
                        depth,
                        name: (*name).to_owned(),
                    }),
            );
            rows.push(FileTreeRow::File {
                depth: directories.len(),
                name: if label == path { name } else { label }.to_string(),
                file,
            });
            previous = directories.to_vec();
        }
        Self { rows }
    }

    pub(crate) fn row_for_file(&self, file: usize) -> Option<usize> {
        self.rows.iter().position(
            |row| matches!(row, FileTreeRow::File { file: candidate, .. } if *candidate == file),
        )
    }

    pub(crate) fn file_at(&self, row: usize) -> Option<usize> {
        match self.rows.get(row)? {
            FileTreeRow::File { file, .. } => Some(*file),
            FileTreeRow::Directory { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FileTree, FileTreeRow};

    #[test]
    fn groups_sorted_paths_under_shared_directories() {
        let tree = FileTree::new(
            [
                ("Cargo.toml", "Cargo.toml"),
                ("crates/app/Cargo.toml", "crates/app/Cargo.toml"),
                ("crates/app/src/main.rs", "crates/app/src/main.rs"),
            ]
            .into_iter(),
        );

        assert_eq!(
            tree.rows,
            [
                FileTreeRow::File {
                    depth: 0,
                    name: "Cargo.toml".to_owned(),
                    file: 0,
                },
                FileTreeRow::Directory {
                    depth: 0,
                    name: "crates".to_owned(),
                },
                FileTreeRow::Directory {
                    depth: 1,
                    name: "app".to_owned(),
                },
                FileTreeRow::File {
                    depth: 2,
                    name: "Cargo.toml".to_owned(),
                    file: 1,
                },
                FileTreeRow::Directory {
                    depth: 2,
                    name: "src".to_owned(),
                },
                FileTreeRow::File {
                    depth: 3,
                    name: "main.rs".to_owned(),
                    file: 2,
                },
            ]
        );
    }

    #[test]
    fn places_a_rename_by_its_real_path_and_keeps_its_label() {
        let tree =
            FileTree::new([("new/dir/file.rs", "old/dir/file.rs => new/dir/file.rs")].into_iter());

        assert_eq!(
            tree.rows,
            [
                FileTreeRow::Directory {
                    depth: 0,
                    name: "new".to_owned(),
                },
                FileTreeRow::Directory {
                    depth: 1,
                    name: "dir".to_owned(),
                },
                FileTreeRow::File {
                    depth: 2,
                    name: "old/dir/file.rs => new/dir/file.rs".to_owned(),
                    file: 0,
                },
            ]
        );
    }
}
