use std::fs;
use std::path::PathBuf;

use notify::event::{CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode};
use notify::{Event, EventKind};
use review_lsp::{FileChange, FileChangeKind};

use super::FileChanges;

fn event(kind: EventKind, paths: &[&PathBuf]) -> Event {
    paths.iter().fold(Event::new(kind), |event, path| {
        event.add_path((*path).clone())
    })
}

#[test]
fn each_change_takes_the_kind_the_disk_shows_when_it_is_sent() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let [
        created,
        edited,
        removed,
        renamed_from,
        renamed_to,
        recreated,
        ignored,
    ] = [
        "created.ex",
        "edited.ex",
        "removed.ex",
        "old.ex",
        "new.ex",
        "recreated.ex",
        "ignored.ex",
    ]
    .map(|name| root.join(name));
    for path in [&created, &edited, &renamed_to, &recreated, &ignored] {
        fs::write(path, "").unwrap();
    }
    let changes = FileChanges::default();
    let includes = |path: &std::path::Path, _| !path.ends_with("ignored.ex");
    for event in [
        event(EventKind::Create(CreateKind::File), &[&created]),
        event(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            &[&created, &edited],
        ),
        event(EventKind::Remove(RemoveKind::File), &[&removed]),
        event(
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            &[&renamed_from, &renamed_to],
        ),
        event(EventKind::Remove(RemoveKind::File), &[&recreated]),
        event(EventKind::Create(CreateKind::File), &[&recreated]),
        event(EventKind::Modify(ModifyKind::Any), &[&root.to_owned()]),
        event(EventKind::Modify(ModifyKind::Any), &[&ignored]),
    ] {
        changes.record(&event, includes);
    }

    let change = |path: &PathBuf, kind| FileChange {
        path: path.clone(),
        kind,
    };
    assert_eq!(
        changes.take(),
        [
            change(&created, FileChangeKind::Created),
            change(&edited, FileChangeKind::Changed),
            change(&renamed_to, FileChangeKind::Created),
            change(&renamed_from, FileChangeKind::Deleted),
            change(&recreated, FileChangeKind::Created),
            change(&removed, FileChangeKind::Deleted),
        ]
    );
    assert!(changes.take().is_empty(), "a change is sent once");
}

#[test]
fn a_directory_that_appears_stands_for_the_files_it_holds() {
    let directory = tempfile::tempdir().unwrap();
    let moved = directory.path().join("lib");
    fs::create_dir_all(moved.join("nested")).unwrap();
    fs::create_dir_all(moved.join("_build")).unwrap();
    for name in ["page.ex", "nested/view.ex", "_build/compiled.ex"] {
        fs::write(moved.join(name), "").unwrap();
    }
    // A link back up is not followed, and not walked around in circles.
    std::os::unix::fs::symlink(directory.path(), moved.join("nested/up")).unwrap();
    let changes = FileChanges::default();
    changes.record(
        &event(
            EventKind::Modify(ModifyKind::Name(RenameMode::To)),
            &[&moved],
        ),
        |path: &std::path::Path, is_dir| !(is_dir && path.ends_with("_build")),
    );

    let paths: Vec<_> = changes
        .take()
        .into_iter()
        .map(|change| (change.path, change.kind))
        .collect();
    assert_eq!(
        paths,
        [
            (moved.join("nested/up"), FileChangeKind::Created),
            (moved.join("nested/view.ex"), FileChangeKind::Created),
            (moved.join("page.ex"), FileChangeKind::Created),
        ]
    );
}
