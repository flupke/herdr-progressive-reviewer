use std::collections::HashSet;

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
        &HashSet::new(),
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
                path: "crates".to_owned(),
                collapsed: false,
            },
            FileTreeRow::Directory {
                depth: 1,
                name: "app".to_owned(),
                path: "crates/app".to_owned(),
                collapsed: false,
            },
            FileTreeRow::File {
                depth: 2,
                name: "Cargo.toml".to_owned(),
                file: 1,
            },
            FileTreeRow::Directory {
                depth: 2,
                name: "src".to_owned(),
                path: "crates/app/src".to_owned(),
                collapsed: false,
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
    let tree = FileTree::new(
        [("new/dir/file.rs", "old/dir/file.rs => new/dir/file.rs")].into_iter(),
        &HashSet::new(),
    );

    assert_eq!(
        tree.rows,
        [
            FileTreeRow::Directory {
                depth: 0,
                name: "new".to_owned(),
                path: "new".to_owned(),
                collapsed: false,
            },
            FileTreeRow::Directory {
                depth: 1,
                name: "dir".to_owned(),
                path: "new/dir".to_owned(),
                collapsed: false,
            },
            FileTreeRow::File {
                depth: 2,
                name: "old/dir/file.rs => new/dir/file.rs".to_owned(),
                file: 0,
            },
        ]
    );
}

#[test]
fn hides_every_descendant_of_a_collapsed_directory() {
    let tree = FileTree::new(
        [
            ("src/app/main.rs", "src/app/main.rs"),
            ("src/lib.rs", "src/lib.rs"),
            ("tests/test.rs", "tests/test.rs"),
        ]
        .into_iter(),
        &HashSet::from(["src".to_owned()]),
    );

    assert_eq!(
        tree.rows,
        [
            FileTreeRow::Directory {
                depth: 0,
                name: "src".to_owned(),
                path: "src".to_owned(),
                collapsed: true,
            },
            FileTreeRow::Directory {
                depth: 0,
                name: "tests".to_owned(),
                path: "tests".to_owned(),
                collapsed: false,
            },
            FileTreeRow::File {
                depth: 1,
                name: "test.rs".to_owned(),
                file: 2,
            },
        ]
    );
}
