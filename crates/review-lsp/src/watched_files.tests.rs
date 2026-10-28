use std::path::Path;

use lsp_types::{FileChangeType, Registration, Unregistration};

use crate::api::{FileChange, FileChangeKind};
use crate::source::path_uri;

use super::WatchedFiles;

fn registration(id: &str, watchers: serde_json::Value) -> Registration {
    let mut options = serde_json::Map::new();
    options.insert("watchers".to_owned(), watchers);
    Registration {
        id: id.to_owned(),
        method: "workspace/didChangeWatchedFiles".to_owned(),
        register_options: Some(options.into()),
    }
}

fn change(path: &Path, kind: FileChangeKind) -> FileChange {
    FileChange {
        path: path.to_owned(),
        kind,
    }
}

/// The paths and types of the events, relative to `root`.
fn events(
    watched: &WatchedFiles,
    root: &Path,
    changes: &[FileChange],
) -> Vec<(String, FileChangeType)> {
    let root = path_uri(root).unwrap();
    watched
        .events(changes)
        .into_iter()
        .map(|event| {
            let path = event
                .uri
                .as_str()
                .strip_prefix(root.as_str())
                .unwrap()
                .to_owned();
            (path, event.typ)
        })
        .collect()
}

#[test]
fn plain_patterns_match_the_absolute_path_or_the_path_from_the_project_root() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut watched = WatchedFiles::new(root);
    watched.register(registration(
        "1",
        serde_json::json!([{ "globPattern": "**/*.{ex,exs}" }, { "globPattern": "config/*.lock" }]),
    ));

    let changes = [
        change(&root.join("apps/web/lib/page.ex"), FileChangeKind::Created),
        change(&root.join("mix.exs"), FileChangeKind::Changed),
        change(&root.join("config/mix.lock"), FileChangeKind::Changed),
        change(&root.join("apps/config/mix.lock"), FileChangeKind::Changed),
        change(&root.join("lib/page.rs"), FileChangeKind::Changed),
    ];
    assert_eq!(
        events(&watched, root, &changes),
        [
            ("/apps/web/lib/page.ex".to_owned(), FileChangeType::CREATED),
            ("/mix.exs".to_owned(), FileChangeType::CHANGED),
            ("/config/mix.lock".to_owned(), FileChangeType::CHANGED),
        ]
    );
}

#[test]
fn a_watcher_hears_only_the_kinds_it_asked_for() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut watched = WatchedFiles::new(root);
    // Delete only.
    watched.register(registration(
        "1",
        serde_json::json!([{ "globPattern": "**/*.ex", "kind": 4 }]),
    ));

    let changes = [
        change(&root.join("created.ex"), FileChangeKind::Created),
        change(&root.join("changed.ex"), FileChangeKind::Changed),
        change(&root.join("gone/deleted.ex"), FileChangeKind::Deleted),
    ];
    assert_eq!(
        events(&watched, root, &changes),
        [("/gone/deleted.ex".to_owned(), FileChangeType::DELETED)]
    );
}

#[test]
fn a_relative_pattern_matches_from_its_base() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let base = root.join("apps/web");
    std::fs::create_dir_all(&base).unwrap();
    let mut watched = WatchedFiles::new(root);
    watched.register(registration(
        "1",
        serde_json::json!([{
            "globPattern": { "baseUri": path_uri(&base).unwrap().as_str(), "pattern": "lib/*.ex" }
        }]),
    ));

    let changes = [
        change(&base.join("lib/page.ex"), FileChangeKind::Changed),
        change(&root.join("lib/page.ex"), FileChangeKind::Changed),
    ];
    assert_eq!(
        events(&watched, root, &changes),
        [("/apps/web/lib/page.ex".to_owned(), FileChangeType::CHANGED)]
    );
}

#[test]
fn unregistering_or_another_method_leaves_no_watcher() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut watched = WatchedFiles::new(root);
    watched.register(registration(
        "1",
        serde_json::json!([{ "globPattern": "**/*.ex" }]),
    ));
    watched.register(Registration {
        method: "textDocument/formatting".to_owned(),
        ..registration("2", serde_json::json!([{ "globPattern": "**/*.rs" }]))
    });
    let changes = [
        change(&root.join("page.ex"), FileChangeKind::Changed),
        change(&root.join("page.rs"), FileChangeKind::Changed),
    ];
    assert_eq!(
        events(&watched, root, &changes),
        [("/page.ex".to_owned(), FileChangeType::CHANGED)]
    );

    watched.unregister(&Unregistration {
        id: "1".to_owned(),
        method: "workspace/didChangeWatchedFiles".to_owned(),
    });
    assert!(watched.events(&changes).is_empty());
}
